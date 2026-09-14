use omawind::{config, engine, fetch, forecast::Forecast, grib, keel, obs, time};
use std::{collections::HashMap, env, path::PathBuf, process::ExitCode};
use tokio::signal::unix::{SignalKind, signal};

const USAGE: &str = "\
usage: omawind run [--offline] [--socket PATH] [--keel PATH]
       omawind fetch
       omawind at [LAT,LON]
       omawind stations
       omawind decode FILE
       omawind --version

run       serve the forecast, the wind at the boat and the stations' measured
          wind to Omahoy apps, fetching a newer HRRR run and NDBC's latest
          reports from NOAA every 10 minutes. --offline uses only what's
          cached. The socket defaults to $XDG_RUNTIME_DIR/omawind/wind.sock
          and omakeel's to $XDG_RUNTIME_DIR/omakeel/keel.sock.
fetch     download the newest HRRR run for the region now.
at        print the cached forecast at a position, or at home.
stations  print the wind the region's stations measured, from NDBC.
decode    list the fields in a GRIB2 file.

Settings: ~/.config/omawind/config.toml (region, home).";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("omawind: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let Some((command, rest)) = args.split_first() else {
        println!("{USAGE}");
        return Ok(());
    };
    match command.as_str() {
        "run" => serve(rest),
        "fetch" if rest.is_empty() => fetch_now(),
        "at" if rest.len() <= 1 => at(rest.first().map(String::as_str)),
        "stations" if rest.is_empty() => stations(),
        "decode" if rest.len() == 1 => decode(&rest[0]),
        "--version" | "version" => {
            println!("omawind {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(format!("unexpected '{}'\n\n{USAGE}", args.join(" "))),
    }
}

fn serve(args: &[String]) -> Result<(), String> {
    let mut socket = None;
    let mut keel_socket = None;
    let mut offline = false;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--offline" => offline = true,
            "--socket" | "--keel" => {
                let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
                if flag == "--socket" {
                    socket = Some(PathBuf::from(value));
                } else {
                    keel_socket = Some(PathBuf::from(value));
                }
            }
            other => return Err(format!("unexpected '{other}'\n\n{USAGE}")),
        }
    }
    let config = engine::Config {
        socket: match socket {
            Some(s) => s,
            None => engine::default_socket().map_err(|e| e.to_string())?,
        },
        keel: keel_socket.or_else(keel::default_socket),
        cache: config::cache_dir(),
        settings: config::config_path(),
        fetch: !offline,
        stations: (!offline).then(obs::Source::ndbc),
        clock: time::now,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let mut terminate = signal(SignalKind::terminate()).map_err(|e| e.to_string())?;
        tokio::select! {
            result = engine::run(config) => result.map_err(|e| e.to_string()),
            _ = tokio::signal::ctrl_c() => Ok(()),
            _ = terminate.recv() => Ok(()),
        }
    })
}

fn settings() -> Result<config::Settings, String> {
    let (s, problems) = config::load(&config::config_path());
    for p in problems {
        eprintln!("omawind: {p}");
    }
    Ok(s)
}

fn fetch_now() -> Result<(), String> {
    let s = settings()?;
    let cache = config::cache_dir();
    let run =
        fetch::newest_run(time::now())?.ok_or("NOMADS lists no HRRR run with 18 hours out yet")?;
    eprintln!("HRRR {} UTC, {} hours out", run.key(), run.unbroken().len());
    let (dir, new) = fetch::download(
        &cache,
        &run,
        &s.region,
        &mut |done, total| {
            eprint!("\rDownloading hour {} of {total}", done + 1);
        },
        &|| false,
    )?;
    if new > 0 {
        eprintln!();
    }
    let f = Forecast::load(&dir)?;
    println!(
        "{}: {} hours, {} to {}",
        dir.display(),
        f.hours.len(),
        time::iso(f.first()),
        time::iso(f.last())
    );
    fetch::prune(&cache, &s.region, &[&dir]);
    Ok(())
}

fn at(position: Option<&str>) -> Result<(), String> {
    let s = settings()?;
    let (lat, lon) = match position {
        None => s.home,
        Some(p) => p
            .split_once(',')
            .and_then(|(a, b)| Some((a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?)))
            .ok_or("expected LAT,LON in degrees, like 37.8663,-122.3148")?,
    };
    let (found, problem) = fetch::load_newest(&config::cache_dir(), &s.region);
    let (_, f) = found.ok_or_else(|| {
        problem.unwrap_or_else(|| "nothing cached for the region yet: run omawind fetch".into())
    })?;
    println!("HRRR {} at {lat:.4}, {lon:.4}", time::iso(f.run));
    println!(
        "{:<21} {:>5} {:>6} {:>6} {:>8}",
        "UTC", "from", "kn", "gust", "hPa"
    );
    let mut any = false;
    for hour in &f.hours {
        let Some(sample) = f.sample(lat, lon, hour.valid) else {
            continue;
        };
        any = true;
        let opt = |v: Option<f64>, places: usize| v.map_or("-".into(), |v| format!("{v:.places$}"));
        println!(
            "{:<21} {:>4}° {:>6.1} {:>6} {:>8}",
            time::iso(hour.valid),
            sample.from_deg.round() as i64 % 360,
            sample.speed_kn,
            opt(sample.gust_kn, 1),
            opt(sample.pressure_hpa, 1)
        );
    }
    if !any {
        return Err("that position is outside the forecast area".into());
    }
    Ok(())
}

fn stations() -> Result<(), String> {
    let s = settings()?;
    let source = obs::Source::ndbc();
    let names = obs::names(&config::cache_dir(), &source).unwrap_or_else(|e| {
        eprintln!("omawind: station names: {e}");
        HashMap::new()
    });
    let all = obs::latest(&source, &names)?;
    let shown: Vec<&obs::Station> = obs::current(&all, s.region, time::now()).collect();
    if shown.is_empty() {
        return Err("no station in the region has reported wind in the last 2 hours".into());
    }
    println!(
        "{:<7} {:<26} {:>5} {:>5} {:>6} {:>6}",
        "NDBC", "", "UTC", "from", "kn", "gust"
    );
    for st in shown {
        let name: String = st.name.as_deref().unwrap_or("").chars().take(26).collect();
        let opt = |v: Option<f64>, f: &dyn Fn(f64) -> String| v.map_or("-".into(), f);
        println!(
            "{:<7} {:<26} {:>5} {:>5} {:>6.1} {:>6}",
            st.id,
            name,
            &time::iso(st.time)[11..16],
            opt(st.from_deg, &|d| format!("{}°", d.round() as i64 % 360)),
            st.speed_kn,
            opt(st.gust_kn, &|g| format!("{g:.1}"))
        );
    }
    Ok(())
}

fn decode(path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let fields = grib::parse(&bytes).map_err(|e| format!("{path}: {e}"))?;
    for f in &fields {
        let kept: Vec<f32> = f.values.iter().copied().filter(|v| !v.is_nan()).collect();
        let min = kept.iter().copied().fold(f32::INFINITY, f32::min);
        let max = kept.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mean = kept.iter().map(|&v| f64::from(v)).sum::<f64>() / kept.len().max(1) as f64;
        println!(
            "{:<6} surface {} level {} run {} valid {} grid {}×{} min {min} max {max} mean {mean:.4}{}",
            f.name(),
            f.surface,
            f.level.map_or("-".into(), |l| l.to_string()),
            time::iso(f.reference),
            time::iso(f.valid()),
            f.grid.nx,
            f.grid.ny,
            if kept.len() < f.values.len() {
                format!(" ({} missing)", f.values.len() - kept.len())
            } else {
                String::new()
            }
        );
    }
    Ok(())
}
