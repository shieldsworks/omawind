#![allow(
    clippy::unwrap_used,
    reason = "a panic is how an integration test fails"
)]

use std::path::PathBuf;
use std::process::Command;

fn helper() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/BarText.js")
}

fn run(script: &str) -> String {
    let output = Command::new("node")
        .arg("-e")
        .arg(script)
        .arg(helper())
        .output()
        .unwrap_or_else(|e| panic!("node is needed to test ui/BarText.js: {e}"));
    assert!(
        output.status.success(),
        "node failed: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn load() -> &'static str {
    r#"
const fs = require("fs");
const src = fs.readFileSync(process.argv[1], "utf8").replace(/^\.pragma library\s*/, "");
const g = {};
new Function("g", src + "\n; g.label = label; g.detail = detail;")(g);
"#
}

fn detail(fields: &str) -> String {
    run(&format!(
        "{load}\nprocess.stdout.write(g.detail({{ {fields} }}));",
        load = load(),
        fields = fields
    ))
}

#[test]
fn the_bar_is_labeled_wind() {
    let out = run(&format!("{}\nprocess.stdout.write(g.label());", load()));
    assert_eq!(out, "Wind");
}

#[test]
fn a_forecast_is_the_tooltip() {
    assert_eq!(
        detail(r#"connected: true, summary: "WSW 14G19 kn""#),
        "WSW 14G19 kn"
    );
}

#[test]
fn a_strong_forecast_mentions_the_advisory() {
    assert_eq!(
        detail(r#"connected: true, summary: "WSW 21 kn", strong: true"#),
        "WSW 21 kn — 21 kn or more: small-craft advisory strength"
    );
}

#[test]
fn connected_with_no_state_yet_is_no_wind_yet() {
    assert_eq!(detail("connected: true"), "No wind yet");
}

#[test]
fn a_closed_socket_says_the_engine_isnt_running() {
    assert_eq!(
        detail("connected: false, waited: true"),
        "omawind isn't running"
    );
}

#[test]
fn a_closed_socket_says_starting_before_it_has_waited() {
    assert_eq!(
        detail("connected: false, waited: false"),
        "Starting omawind…"
    );
}

#[test]
fn another_protocol_version_is_called_out() {
    assert_eq!(
        detail("incompatible: true, connected: true"),
        "omawind speaks another protocol version"
    );
}

#[test]
fn a_download_in_progress_is_called_out() {
    assert_eq!(
        detail("connected: true, downloading: true"),
        "Downloading forecast…"
    );
}
