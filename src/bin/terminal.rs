//! # lulu-terminal — Span, Scenario & Step demo
//!
//! This binary demonstrates the **test-scenario** and **step** helpers from
//! `lulu-logs-client` and shows how they render on the terminal via the
//! built-in `terminal_logger`.
//!
//! The terminal output uses ANSI colours and Unicode symbols:
//!
//! ```text
//! ▶ scenario-name              — scenario started
//! ✓ scenario-name              — scenario passed  (green)
//! ✗ scenario-name — error …    — scenario failed  (red)
//!   ▸ step-name                — step started     (cyan)
//!   ✓ step-name                — step passed      (green)
//!   ✗ step-name — error …      — step failed      (red)
//! ```
//!
//! # Usage
//!
//! ```bash
//! cargo run --bin lulu-terminal                     # default broker 127.0.0.1:1883
//! cargo run --bin lulu-terminal -- 192.168.1.10 1883
//! ```

use std::thread;
use std::time::Duration;

use lulu_logs_client::{
    lulu_init, lulu_scenario, lulu_shutdown, lulu_span, lulu_stats, Data, LuluConfig, LuluPublisher,
};
use serde_json::json;

// ─── CLI ──────────────────────────────────────────────────────────────────────

struct Args {
    broker_host: String,
    broker_port: u16,
}

fn parse_args() -> Args {
    let mut iter = std::env::args().skip(1);
    let broker_host = iter.next().unwrap_or_else(|| "127.0.0.1".to_string());
    let broker_port = iter.next().and_then(|p| p.parse().ok()).unwrap_or(1883u16);
    Args {
        broker_host,
        broker_port,
    }
}

/// Small pause so the terminal output appears progressively.
fn pace() {
    thread::sleep(Duration::from_millis(120));
}

// ─── Scenarios ────────────────────────────────────────────────────────────────

/// Scenario 1 — all steps pass → scenario succeeds.
fn scenario_voltage_regulation() {
    let voltage = LuluPublisher::new("psu/channel-1", "voltage")
        .unwrap()
        .terminal(true);

    // ── Begin scenario ────────────────────────────────────────────────────
    let scenario = lulu_scenario("voltage-regulation-3v3").unwrap();
    pace();

    // ── Step 1: set voltage ───────────────────────────────────────────────
    let step1_meta = json!({"target_v": 3.3});
    let step1 = scenario
        .step_with_metadata("set-voltage", Some(&step1_meta))
        .unwrap();
    pace();

    // Simulate the action — publish real measurement data
    let _ = voltage.info(Data::Float32(3.31));
    pace();

    let step1_result = json!({"actual_v": 3.31});
    let _ = step1.end(true, None, Some(42), Some(&step1_meta), Some(&step1_result));
    pace();

    // ── Step 2: verify stability ──────────────────────────────────────────
    let step2_meta = json!({"samples": 10, "tolerance_mv": 50});
    let step2 = scenario
        .step_with_metadata("verify-stability", Some(&step2_meta))
        .unwrap();
    pace();

    let _ = voltage.info(Data::Float32(3.30));
    let _ = voltage.info(Data::Float32(3.29));
    pace();

    let step2_result = json!({"min_v": 3.29, "max_v": 3.31, "ripple_mv": 20});
    let _ = step2.end(true, None, Some(85), Some(&step2_meta), Some(&step2_result));
    pace();

    // ── End scenario — success ────────────────────────────────────────────
    let _ = scenario.end(true, None);
    pace();
}

/// Scenario 2 — second step fails → scenario fails.
fn scenario_overcurrent_protection() {
    let current = LuluPublisher::new("psu/channel-1", "current")
        .unwrap()
        .terminal(true);

    // ── Begin scenario ────────────────────────────────────────────────────
    let scenario = lulu_scenario("overcurrent-protection").unwrap();
    pace();

    // ── Step 1: ramp current (passes) ─────────────────────────────────────
    let step1_meta = json!({"ramp_target_a": 0.95, "limit_a": 1.0});
    let step1 = scenario
        .step_with_metadata("ramp-current", Some(&step1_meta))
        .unwrap();
    pace();

    let _ = current.info(Data::Float32(0.45));
    let _ = current.warn(Data::Float32(0.95));
    pace();

    let step1_result = json!({"peak_a": 0.95});
    let _ = step1.end(true, None, Some(30), Some(&step1_meta), Some(&step1_result));
    pace();

    // ── Step 2: trigger protection (fails) ────────────────────────────────
    let step2_meta = json!({"inject_a": 1.05, "trip_timeout_ms": 100});
    let step2 = scenario
        .step_with_metadata("trigger-protection", Some(&step2_meta))
        .unwrap();
    pace();

    let _ = current.error(Data::Float32(1.05));
    pace();

    let step2_result = json!({"peak_a": 1.05, "protection_triggered": false});
    let _ = step2.end(
        false,
        Some("protection did not trigger within 100ms"),
        Some(105),
        Some(&step2_meta),
        Some(&step2_result),
    );
    pace();

    // ── End scenario — failure ────────────────────────────────────────────
    let _ = scenario.end(false, Some("current reached 1.05A without tripping"));
    pace();
}

/// Scenario 3 — started but never ended (in-progress / pending).
fn scenario_signal_integrity() {
    let frequency = LuluPublisher::new("oscilloscope/probe-a", "frequency")
        .unwrap()
        .terminal(true);

    // ── Begin scenario ────────────────────────────────────────────────────
    let _scenario = lulu_scenario("signal-integrity-check").unwrap();
    pace();

    // ── Step: measure frequency (started, never completed) ────────────────
    let step_meta = json!({"expected_hz": 1_000_000});
    let _step = _scenario
        .step_with_metadata("measure-frequency", Some(&step_meta))
        .unwrap();
    pace();

    let _ = frequency.info(Data::Float64(1_000_000.0));
    pace();

    // Intentionally no .end() — handles dropped, left in progress.
}

/// Demo 4 — String attribute: simple status messages.
fn demo_string_messages() {
    let status = LuluPublisher::new("psu/channel-1", "status")
        .unwrap()
        .terminal(true);

    let _ = status.info(Data::String("Power supply initialised".into()));
    pace();
    let _ = status.info(Data::String("Output enabled — CV mode".into()));
    pace();
    let _ = status.warn(Data::String("Temperature rising: 62°C".into()));
    pace();
    let _ = status.error(Data::String("Over-temperature protection tripped".into()));
    pace();
}

/// Demo 5 — Generic span: calibration routine.
fn demo_generic_span() {
    let calibration = LuluPublisher::new("psu/channel-1", "calibration")
        .unwrap()
        .terminal(true);

    // Start a generic span of kind "calibration".
    let meta = json!({"reference_v": 5.0, "points": 3});
    let mut span = lulu_span("5V-calibration")
        .source("psu/channel-1")
        .attribute("calibration")
        .kind("calibration")
        .metadata(&meta)
        .terminal(true)
        .begin()
        .unwrap();
    pace();

    // Publish measurements during calibration.
    let _ = calibration.info(Data::Float32(4.98));
    pace();
    let _ = calibration.info(Data::Float32(5.01));
    pace();
    let _ = calibration.info(Data::Float32(5.00));
    pace();

    // End the span.
    span.set_result(&json!({"avg_v": 4.997, "deviation_mv": 15}));
    span.set_duration_ms(320);
    let _ = span.end();
    pace();
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let args = parse_args();

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  lulu-terminal — Scenario & Step terminal rendering     ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();
    println!("  broker : {}:{}", args.broker_host, args.broker_port);
    println!();
    println!("  This binary demonstrates how lulu_scenario and");
    println!("  lulu_step return handles whose .end() method");
    println!("  renders on the terminal via the built-in terminal_logger.");
    println!();
    println!("  Legend:");
    println!("    ▶  scenario started");
    println!("    \x1b[32m✓\x1b[0m  passed  (green)");
    println!("    \x1b[31m✗\x1b[0m  failed  (red)");
    println!("    \x1b[36m▸\x1b[0m  step started (cyan, indented)");
    println!();

    // ── Initialise ────────────────────────────────────────────────────────
    let config = LuluConfig {
        broker_host: args.broker_host,
        broker_port: args.broker_port,
        client_id_prefix: "lulu-terminal".to_string(),
        queue_capacity: 256,
        keep_alive_secs: 5,
        terminal_logger: true,
    };

    if let Err(e) = lulu_init(config) {
        eprintln!("[ERROR] lulu_init failed: {e}");
        std::process::exit(1);
    }

    // Brief pause for MQTT handshake (non-blocking — logs queue regardless).
    thread::sleep(Duration::from_millis(300));

    // ── Scenario 1: all steps pass ────────────────────────────────────────
    scenario_voltage_regulation();

    // ── Scenario 2: one step fails ────────────────────────────────────────
    scenario_overcurrent_protection();

    // ── Scenario 3: left in progress ──────────────────────────────────────
    scenario_signal_integrity();

    // ── Demo 4: String messages ───────────────────────────────────────────
    demo_string_messages();

    // ── Demo 5: generic span ──────────────────────────────────────────────
    demo_generic_span();

    // ── Stats ─────────────────────────────────────────────────────────────
    if let Some(stats) = lulu_stats() {
        println!("── Stats ──────────────────────────────────────────────────");
        println!("  published : {}", stats.messages_published);
        println!("  dropped   : {}", stats.messages_dropped);
        println!("  queued    : {}", stats.queue_current_size);
        println!("  reconnect : {}", stats.reconnections);
        println!("────────────────────────────────────────────────────────────");
        println!();
    }

    // ── Shutdown ──────────────────────────────────────────────────────────
    println!("[shutdown] draining queue…");
    lulu_shutdown();
    println!("[shutdown] done");
}
