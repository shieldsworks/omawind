#![allow(
    clippy::unwrap_used,
    reason = "a panic is how an integration test fails"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/hrrr/2026091403"
);

fn command(args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_omawind"));
    cmd.args(args)
        .env_remove("HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("XDG_RUNTIME_DIR")
        .env_remove("OMAWIND_CONFIG");
    cmd
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("omawind-paths-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn wait_or_kill(cmd: &mut Command, limit: Duration) -> Output {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + limit;
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return child.wait_with_output().unwrap();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn assert_failure(output: &Output, stderr: &str) {
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        stderr,
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn at_reports_when_home_is_not_set() {
    let output = wait_or_kill(&mut command(&["at"]), Duration::from_secs(5));
    assert_failure(&output, "omawind: HOME is not set\n");
}

#[test]
fn at_rejects_a_relative_home() {
    let output = wait_or_kill(command(&["at"]).env("HOME", "rel"), Duration::from_secs(5));
    assert_failure(&output, "omawind: HOME must be an absolute path\n");
}

#[test]
fn at_rejects_a_relative_xdg_config_home() {
    let dir = scratch("config-rel");
    let output = wait_or_kill(
        command(&["at"])
            .env("HOME", &dir)
            .env("XDG_CONFIG_HOME", "rel"),
        Duration::from_secs(5),
    );
    assert_failure(
        &output,
        "omawind: XDG_CONFIG_HOME must be an absolute path\n",
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn at_rejects_a_relative_xdg_cache_home() {
    let dir = scratch("cache-rel");
    let config = dir.join("config");
    fs::create_dir_all(&config).unwrap();
    let output = wait_or_kill(
        command(&["at"])
            .env("HOME", &dir)
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_CACHE_HOME", "rel"),
        Duration::from_secs(5),
    );
    assert_failure(
        &output,
        "omawind: XDG_CACHE_HOME must be an absolute path\n",
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn at_uses_absolute_xdg_dirs_when_home_is_unset() {
    let dir = scratch("xdg-only");
    let config = dir.join("config");
    let cache = dir.join("cache");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&cache).unwrap();
    let output = wait_or_kill(
        command(&["at"])
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_CACHE_HOME", &cache),
        Duration::from_secs(5),
    );
    assert_failure(
        &output,
        "omawind: nothing cached for the region yet: run omawind fetch\n",
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn at_reads_the_cache_under_home_when_xdg_dirs_are_empty() {
    let dir = scratch("empty-xdg");
    let run = dir.join(".cache/omawind/hrrr/36.8000_-123.8000_38.8000_-121.6000/2026091403");
    fs::create_dir_all(&run).unwrap();
    for hour in ["f00.grib2", "f01.grib2", "f02.grib2"] {
        fs::copy(Path::new(FIXTURE).join(hour), run.join(hour)).unwrap();
    }
    let output = wait_or_kill(
        command(&["at", "37.8663,-122.3148"])
            .env("HOME", &dir)
            .env("XDG_CONFIG_HOME", "")
            .env("XDG_CACHE_HOME", ""),
        Duration::from_secs(30),
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let first = stdout.lines().next().unwrap_or("");
    assert_eq!(first, "HRRR 2026-09-14T03:00:00Z at 37.8663, -122.3148");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn run_exits_when_the_runtime_dir_is_unset() {
    let dir = scratch("run-unset");
    let config = dir.join("config");
    let cache = dir.join("cache");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&cache).unwrap();
    let output = wait_or_kill(
        command(&["run", "--offline"])
            .env("HOME", &dir)
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_CACHE_HOME", &cache),
        Duration::from_secs(5),
    );
    assert_failure(
        &output,
        "omawind: XDG_RUNTIME_DIR must be set to an absolute path\n",
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn run_rejects_a_relative_runtime_dir() {
    let dir = scratch("run-rel");
    let config = dir.join("config");
    let cache = dir.join("cache");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&cache).unwrap();
    let output = wait_or_kill(
        command(&["run", "--offline"])
            .env("HOME", &dir)
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_CACHE_HOME", &cache)
            .env("XDG_RUNTIME_DIR", "rel"),
        Duration::from_secs(5),
    );
    assert_failure(
        &output,
        "omawind: XDG_RUNTIME_DIR must be an absolute path\n",
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn run_rejects_a_relative_runtime_dir_even_with_socket() {
    let dir = scratch("run-socket-rel");
    let config = dir.join("config");
    let cache = dir.join("cache");
    let sock = dir.join("wind.sock");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&cache).unwrap();
    let mut child = command(&["run", "--offline", "--socket", sock.to_str().unwrap()])
        .env("HOME", &dir)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_CACHE_HOME", &cache)
        .env("XDG_RUNTIME_DIR", "rel")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_socket = false;
    loop {
        if sock.exists() {
            saw_socket = true;
            let _ = child.kill();
            break;
        }
        if child.try_wait().unwrap().is_some() || Instant::now() >= deadline {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    assert_failure(
        &output,
        "omawind: XDG_RUNTIME_DIR must be an absolute path\n",
    );
    assert!(!saw_socket);
    assert!(!sock.exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn run_stays_up_with_socket_when_the_runtime_dir_is_unset() {
    let dir = scratch("run-socket-ok");
    let config = dir.join("config");
    let cache = dir.join("cache");
    let sock = dir.join("wind.sock");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&cache).unwrap();
    let mut child = command(&["run", "--offline", "--socket", sock.to_str().unwrap()])
        .env("HOME", &dir)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_CACHE_HOME", &cache)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !sock.exists() {
        match child.try_wait().unwrap() {
            Some(status) => panic!("exited {status} before the socket appeared"),
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("socket did not appear");
            }
        }
    }
    assert!(child.try_wait().unwrap().is_none());
    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn at_with_omawind_config_does_not_need_home() {
    let dir = scratch("override");
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).unwrap();
    let config = dir.join("missing.toml");
    let output = wait_or_kill(
        command(&["at"])
            .env("OMAWIND_CONFIG", &config)
            .env("XDG_CACHE_HOME", &cache),
        Duration::from_secs(5),
    );
    assert_failure(
        &output,
        "omawind: nothing cached for the region yet: run omawind fetch\n",
    );
    let _ = fs::remove_dir_all(dir);
}
