//! # lulu-inject — Test log injector
//!
//! This binary serves two purposes:
//!
//! 1. **Programming example** — each section is heavily commented to show how
//!    to use the `lulu-logs-client` library (init → publish → stats → shutdown).
//!
//! 2. **UI test data injector** — sends ~50 realistic log entries across
//!    multiple instruments, log levels, and data types so the
//!    hw-tests-log-app UI can be exercised with live MQTT data.
//!
//! # Usage
//!
//! ```
//! cargo run                            # connects to 127.0.0.1:1883 (default)
//! cargo run -- 192.168.1.10 1883       # connect to a remote broker
//! ```

use std::time::Duration;

use lulu_logs_client::{
    lulu_init, lulu_is_connected, lulu_publish, lulu_scenario, lulu_shutdown, lulu_span,
    lulu_start_pulse, lulu_stats, lulu_stop_pulse, lulu_tool_call_beg, lulu_tool_call_end, Data,
    LogLevel, LuluConfig,
};
use serde_json::json;

// ─── CLI argument parsing ─────────────────────────────────────────────────────

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

// ─── Scenario definition ──────────────────────────────────────────────────────

/// One log entry to inject into the system.
struct Scenario {
    /// MQTT source path — segments must match `[a-z0-9-]+`, separated by `/`.
    source: &'static str,
    /// Attribute name — must not contain `/` and must be non-empty.
    attribute: &'static str,
    /// Severity level (Trace / Debug / Info / Warn / Error / Fatal).
    level: LogLevel,
    /// Typed payload — carries both the wire type and the native value.
    data: Data,
    /// Human-readable description printed during injection.
    description: &'static str,
}

fn build_scenarios() -> Vec<Scenario> {
    vec![
        // ── Power supply — channel 1 ─────────────────────────────────────────
        Scenario {
            source: "psu/channel-1",
            attribute: "voltage",
            level: LogLevel::Trace,
            data: Data::Float32(0.0),
            description: "PSU ch1 voltage pre-init",
        },
        Scenario {
            source: "psu/channel-1",
            attribute: "voltage",
            level: LogLevel::Debug,
            data: Data::Float32(3.29),
            description: "PSU ch1 voltage ramp-up",
        },
        Scenario {
            source: "psu/channel-1",
            attribute: "voltage",
            level: LogLevel::Info,
            data: Data::Float32(3.3),
            description: "PSU ch1 voltage nominal",
        },
        Scenario {
            source: "psu/channel-1",
            attribute: "current",
            level: LogLevel::Info,
            data: Data::Float32(0.45),
            description: "PSU ch1 current nominal",
        },
        Scenario {
            source: "psu/channel-1",
            attribute: "current",
            level: LogLevel::Warn,
            data: Data::Float32(0.98),
            description: "PSU ch1 current near limit",
        },
        Scenario {
            source: "psu/channel-1",
            attribute: "current",
            level: LogLevel::Error,
            data: Data::Float32(1.05),
            description: "PSU ch1 overcurrent detected",
        },
        Scenario {
            source: "psu/channel-1",
            attribute: "status",
            level: LogLevel::Info,
            data: Data::String("CC".to_string()),
            description: "PSU ch1 in constant-current mode",
        },
        Scenario {
            source: "psu/channel-1",
            attribute: "status",
            level: LogLevel::Fatal,
            data: Data::String("FAULT".to_string()),
            description: "PSU ch1 fault condition",
        },
        Scenario {
            source: "psu/channel-1",
            attribute: "enabled",
            level: LogLevel::Debug,
            data: Data::Bool(true),
            description: "PSU ch1 output enabled",
        },
        Scenario {
            source: "psu/channel-1",
            attribute: "enabled",
            level: LogLevel::Info,
            data: Data::Bool(false),
            description: "PSU ch1 output disabled",
        },
        Scenario {
            source: "psu/channel-1",
            attribute: "report",
            level: LogLevel::Info,
            data: Data::Json(r#"{"voltage":3.3,"current":0.45,"mode":"CV","enabled":true}"#.to_string()),
            description: "PSU ch1 full status report",
        },
        // ── Power supply — channel 2 ─────────────────────────────────────────
        Scenario {
            source: "psu/channel-2",
            attribute: "voltage",
            level: LogLevel::Info,
            data: Data::Float32(5.0),
            description: "PSU ch2 voltage nominal",
        },
        Scenario {
            source: "psu/channel-2",
            attribute: "voltage",
            level: LogLevel::Warn,
            data: Data::Float32(5.21),
            description: "PSU ch2 voltage overshoot",
        },
        Scenario {
            source: "psu/channel-2",
            attribute: "current",
            level: LogLevel::Info,
            data: Data::Float32(0.12),
            description: "PSU ch2 current low",
        },
        Scenario {
            source: "psu/channel-2",
            attribute: "status",
            level: LogLevel::Debug,
            data: Data::String("CV".to_string()),
            description: "PSU ch2 constant-voltage mode",
        },
        // ── Oscilloscope — probe A ────────────────────────────────────────────
        Scenario {
            source: "oscilloscope/probe-a",
            attribute: "frequency",
            level: LogLevel::Info,
            data: Data::Float64(1_000_000.0),
            description: "Probe A signal 1 MHz",
        },
        Scenario {
            source: "oscilloscope/probe-a",
            attribute: "frequency",
            level: LogLevel::Debug,
            data: Data::Float64(999_847.3),
            description: "Probe A freq fine reading",
        },
        Scenario {
            source: "oscilloscope/probe-a",
            attribute: "amplitude",
            level: LogLevel::Info,
            data: Data::Float32(1.8),
            description: "Probe A amplitude nominal",
        },
        Scenario {
            source: "oscilloscope/probe-a",
            attribute: "amplitude",
            level: LogLevel::Warn,
            data: Data::Float32(3.7),
            description: "Probe A amplitude high",
        },
        Scenario {
            source: "oscilloscope/probe-a",
            attribute: "clipped",
            level: LogLevel::Error,
            data: Data::Bool(true),
            description: "Probe A input clipping",
        },
        Scenario {
            source: "oscilloscope/probe-a",
            attribute: "clipped",
            level: LogLevel::Info,
            data: Data::Bool(false),
            description: "Probe A no clipping",
        },
        Scenario {
            source: "oscilloscope/probe-a",
            attribute: "sample-count",
            level: LogLevel::Trace,
            data: Data::Int32(1024),
            description: "Probe A sample buffer 1 K",
        },
        Scenario {
            source: "oscilloscope/probe-a",
            attribute: "sample-count",
            level: LogLevel::Debug,
            data: Data::Int32(2048),
            description: "Probe A sample buffer 2 K",
        },
        Scenario {
            source: "oscilloscope/probe-a",
            attribute: "report",
            level: LogLevel::Debug,
            data: Data::Json(
                r#"{"freq_hz":1000000.0,"amplitude_v":1.8,"clipped":false,"samples":1024}"#.to_string(),
            ),
            description: "Probe A full report",
        },
        // ── Oscilloscope — probe B ────────────────────────────────────────────
        Scenario {
            source: "oscilloscope/probe-b",
            attribute: "frequency",
            level: LogLevel::Info,
            data: Data::Float64(50.0),
            description: "Probe B mains 50 Hz",
        },
        Scenario {
            source: "oscilloscope/probe-b",
            attribute: "amplitude",
            level: LogLevel::Info,
            data: Data::Float32(230.0),
            description: "Probe B mains 230 V rms",
        },
        Scenario {
            source: "oscilloscope/probe-b",
            attribute: "amplitude",
            level: LogLevel::Fatal,
            data: Data::Float32(265.0),
            description: "Probe B mains overvoltage!",
        },
        // ── Multimeter — DC ───────────────────────────────────────────────────
        Scenario {
            source: "multimeter/dc",
            attribute: "voltage",
            level: LogLevel::Trace,
            data: Data::Float64(0.0),
            description: "DMM zeroing",
        },
        Scenario {
            source: "multimeter/dc",
            attribute: "voltage",
            level: LogLevel::Debug,
            data: Data::Float64(3.297),
            description: "DMM voltage reading",
        },
        Scenario {
            source: "multimeter/dc",
            attribute: "voltage",
            level: LogLevel::Info,
            data: Data::Float64(3.300),
            description: "DMM voltage nominal",
        },
        Scenario {
            source: "multimeter/dc",
            attribute: "voltage",
            level: LogLevel::Warn,
            data: Data::Float64(3.401),
            description: "DMM voltage slightly high",
        },
        Scenario {
            source: "multimeter/dc",
            attribute: "resistance",
            level: LogLevel::Info,
            data: Data::Float64(10_000.0),
            description: "DMM resistance 10 kΩ",
        },
        Scenario {
            source: "multimeter/dc",
            attribute: "resistance",
            level: LogLevel::Warn,
            data: Data::Float64(10_850.0),
            description: "DMM resistance out of tolerance",
        },
        Scenario {
            source: "multimeter/dc",
            attribute: "continuity",
            level: LogLevel::Info,
            data: Data::Bool(true),
            description: "DMM continuity OK",
        },
        Scenario {
            source: "multimeter/dc",
            attribute: "continuity",
            level: LogLevel::Error,
            data: Data::Bool(false),
            description: "DMM continuity FAIL",
        },
        Scenario {
            source: "multimeter/dc",
            attribute: "range",
            level: LogLevel::Debug,
            data: Data::String("auto".to_string()),
            description: "DMM auto-range selected",
        },
        Scenario {
            source: "multimeter/dc",
            attribute: "overload",
            level: LogLevel::Error,
            data: Data::Bool(true),
            description: "DMM input overload",
        },
        Scenario {
            source: "multimeter/dc",
            attribute: "report",
            level: LogLevel::Info,
            data: Data::Json(
                r#"{"voltage_v":3.3,"resistance_ohm":10000,"continuity":true,"range":"auto"}"#.to_string(),
            ),
            description: "DMM full report",
        },
        // ── Function generator — output 1 ────────────────────────────────────
        Scenario {
            source: "funcgen/output-1",
            attribute: "waveform",
            level: LogLevel::Debug,
            data: Data::String("sine".to_string()),
            description: "Funcgen waveform type",
        },
        Scenario {
            source: "funcgen/output-1",
            attribute: "frequency",
            level: LogLevel::Info,
            data: Data::Float64(10_000.0),
            description: "Funcgen 10 kHz",
        },
        Scenario {
            source: "funcgen/output-1",
            attribute: "amplitude",
            level: LogLevel::Info,
            data: Data::Float32(1.0),
            description: "Funcgen 1 Vpp",
        },
        Scenario {
            source: "funcgen/output-1",
            attribute: "offset",
            level: LogLevel::Trace,
            data: Data::Float32(0.0),
            description: "Funcgen DC offset zero",
        },
        Scenario {
            source: "funcgen/output-1",
            attribute: "enabled",
            level: LogLevel::Info,
            data: Data::Bool(true),
            description: "Funcgen output on",
        },
        // ── Test session metadata ─────────────────────────────────────────────
        Scenario {
            source: "session/run-001",
            attribute: "name",
            level: LogLevel::Info,
            data: Data::String("voltage-sweep-test".to_string()),
            description: "Test session name",
        },
        Scenario {
            source: "session/run-001",
            attribute: "step",
            level: LogLevel::Info,
            data: Data::Int32(1),
            description: "Test step 1",
        },
        Scenario {
            source: "session/run-001",
            attribute: "step",
            level: LogLevel::Info,
            data: Data::Int32(2),
            description: "Test step 2",
        },
        Scenario {
            source: "session/run-001",
            attribute: "result",
            level: LogLevel::Info,
            data: Data::String("PASS".to_string()),
            description: "Test result PASS",
        },
        Scenario {
            source: "session/run-001",
            attribute: "result",
            level: LogLevel::Error,
            data: Data::String("FAIL".to_string()),
            description: "Test result FAIL",
        },
        Scenario {
            source: "session/run-001",
            attribute: "summary",
            level: LogLevel::Info,
            data: Data::Json(
                r#"{"name":"voltage-sweep-test","steps":2,"passed":1,"failed":1,"duration_ms":4200}"#.to_string(),
            ),
            description: "Test session summary",
        },
        // ── Serial terminal (u-boot) ──────────────────────────────────────────
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Debug,
            data: Data::Bytes(b"U-Boot 2023.07 (Jan 01 2024 - 12:00:00 +0000)\r\n".to_vec()),
            description: "U-Boot banner received",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Info,
            data: Data::Bytes(b"SoC: AM335x\r\n".to_vec()),
            description: "SoC identification",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Info,
            data: Data::Bytes(b"Model: Beaglebone Black\r\n".to_vec()),
            description: "Device model",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Info,
            data: Data::Bytes(b"DRAM: 512 MiB\r\n".to_vec()),
            description: "DRAM size",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Debug,
            data: Data::Bytes(b"Loading Environment from MMC...\r\n".to_vec()),
            description: "Environment loading",
        },
        Scenario {
            source: "serial",
            attribute: "TX",
            level: LogLevel::Debug,
            data: Data::Bytes(b"env\r\n".to_vec()),
            description: "User command: env",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Debug,
            data: Data::Bytes(b"baudrate=115200\r\n".to_vec()),
            description: "Environment variable (baudrate)",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Debug,
            data: Data::Bytes(b"bootargs=console=ttyO0,115200n8 root=/dev/mmcblk0p2 rw\r\n".to_vec()),
            description: "Environment variable (bootargs)",
        },
        Scenario {
            source: "serial",
            attribute: "TX",
            level: LogLevel::Debug,
            data: Data::Bytes(b"printenv ethaddr\r\n".to_vec()),
            description: "User command: printenv ethaddr",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Info,
            data: Data::Bytes(b"ethaddr=d8:3d:c8:XX:XX:XX\r\n".to_vec()),
            description: "MAC address response",
        },
        Scenario {
            source: "serial",
            attribute: "TX",
            level: LogLevel::Debug,
            data: Data::Bytes(b"boot\r\n".to_vec()),
            description: "User command: boot",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Info,
            data: Data::Bytes(b"MMC: Waiting for init complete...\r\n".to_vec()),
            description: "MMC initialization",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Debug,
            data: Data::Bytes(b"Loading Image from MMC...\r\n".to_vec()),
            description: "Kernel image loading",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Info,
            data: Data::Bytes(b"Booting Linux...\r\n".to_vec()),
            description: "Linux kernel boot",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Debug,
            data: Data::Bytes(b"Linux version 5.10.0-1017-beagle (build@workstation) (gcc-10, GNU ld (GNU Binutils) 2.36.1)\r\n".to_vec()),
            description: "Linux kernel version",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Info,
            data: Data::Bytes(b"Welcome to Embedded Linux\n\x07".to_vec()),
            description: "System boot complete",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Trace,
            data: Data::Bytes(b"root@beaglebone:~# ".to_vec()),
            description: "Serial prompt displayed",
        },
        Scenario {
            source: "serial",
            attribute: "TX",
            level: LogLevel::Debug,
            data: Data::Bytes([0x03].to_vec()),  // Ctrl+C
            description: "User sends Ctrl+C",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Debug,
            data: Data::Bytes(b"^C\r\n".to_vec()),
            description: "Echo of Ctrl+C received",
        },
        Scenario {
            source: "serial",
            attribute: "RX",
            level: LogLevel::Info,
            data: Data::Json(r#"{"timestamp":"2024-01-01T12:00:45.123Z","bytes":256,"errors":0,"baudrate":115200}"#.to_string()),
            description: "RX statistics",
        },
        // ── Network capture ───────────────────────────────────────────────────
        Scenario {
            source: "network/eth0",
            attribute: "rx",
            level: LogLevel::Debug,
            // Minimal Ethernet frame header (dst MAC, src MAC, EtherType IPv4)
            data: Data::NetPacket(vec![
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // dst: broadcast
                0xD8, 0x3D, 0xC8, 0x00, 0x01, 0x02, // src: d8:3d:c8:00:01:02
                0x08, 0x00,                           // EtherType: IPv4
            ]),
            description: "Ethernet broadcast frame (header only)",
        },
        Scenario {
            source: "network/eth0",
            attribute: "tx",
            level: LogLevel::Trace,
            data: Data::NetPacket(vec![
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, // dst MAC
                0xD8, 0x3D, 0xC8, 0x00, 0x01, 0x02, // src MAC
                0x08, 0x00,                           // EtherType: IPv4
                0x45, 0x00, 0x00, 0x28,               // IP: version/IHL, DSCP, total length 40
                0x00, 0x01, 0x00, 0x00,               // IP: identification, flags, fragment offset
                0x40, 0x11, 0x00, 0x00,               // IP: TTL=64, proto=UDP, checksum placeholder
                0xC0, 0xA8, 0x01, 0x01,               // IP src: 192.168.1.1
                0xC0, 0xA8, 0x01, 0x02,               // IP dst: 192.168.1.2
            ]),
            description: "IPv4/UDP packet (header)",
        },
        // ── Serial link chunks ────────────────────────────────────────────────
        Scenario {
            source: "serial/uart-1",
            attribute: "rx",
            level: LogLevel::Debug,
            data: Data::SerialChunk(b"AT+GMR\r\n".to_vec()),
            description: "AT command received on UART-1",
        },
        Scenario {
            source: "serial/uart-1",
            attribute: "tx",
            level: LogLevel::Debug,
            data: Data::SerialChunk(b"AT+GMR\r\nOK\r\n".to_vec()),
            description: "AT response sent on UART-1",
        },
        Scenario {
            source: "serial/uart-1",
            attribute: "rx",
            level: LogLevel::Info,
            // Binary frame with non-printable bytes (e.g. a Modbus RTU response)
            data: Data::SerialChunk(vec![0x01, 0x03, 0x02, 0x00, 0x17, 0xF8, 0x4A]),
            description: "Modbus RTU response on UART-1",
        },
    ]
}

/// Injects scenario and span examples using the dedicated lifecycle helpers.
fn inject_test_scenarios() {
    println!("─── Spans and Scenarios (lulu-logs v1.3.0 §3.4) ──────");

    // ── Generic span: calibration window ─────────────────────────────────
    let generic_source = "calibration/station-a";
    let generic_attr = "span";

    print_scenario_step(
        "BEG",
        generic_source,
        generic_attr,
        "generic span calibration-window",
    );
    let generic_metadata = json!({"operator":"alice","batch":"2026-03-a"});
    let mut span = lulu_span("calibration-window")
        .source(generic_source)
        .attribute(generic_attr)
        .kind("calibration")
        .metadata(&generic_metadata)
        .begin()
        .unwrap();
    std::thread::sleep(Duration::from_millis(10));

    print_scenario_step(
        "END",
        generic_source,
        generic_attr,
        "generic span calibration-window → SUCCESS",
    );
    span.set_metadata(&generic_metadata);
    span.set_result(&json!({"calibrated_channels": 4}));
    span.set_duration_ms(84);
    let _ = span.end();
    std::thread::sleep(Duration::from_millis(300));

    // ── Scenario 1: passing test ──────────────────────────────────────────
    let name = "voltage-regulation-3v3";

    print_scenario_step("BEG", "test", "scenario", name);
    let scenario1 = lulu_scenario(name).unwrap();
    std::thread::sleep(Duration::from_millis(10));

    // Some normal logs in between (these belong to the scenario)
    let _ = lulu_publish(
        "psu/channel-1",
        "voltage",
        LogLevel::Info,
        Data::Float32(3.30),
    );
    println!("      ↳ psu/channel-1/voltage = 3.30 (float32)");
    std::thread::sleep(Duration::from_millis(10));

    let _ = lulu_publish(
        "psu/channel-1",
        "voltage",
        LogLevel::Info,
        Data::Float32(3.31),
    );
    println!("      ↳ psu/channel-1/voltage = 3.31 (float32)");
    std::thread::sleep(Duration::from_millis(10));

    print_scenario_step("END", "test", "scenario", &format!("{} → SUCCESS", name));
    let _ = scenario1.end(true, None);
    std::thread::sleep(Duration::from_millis(300));

    // ── Scenario 2: failing test ──────────────────────────────────────────
    let name2 = "overcurrent-protection";

    print_scenario_step("BEG", "test", "scenario", name2);
    let scenario2 = lulu_scenario(name2).unwrap();
    std::thread::sleep(Duration::from_millis(10));

    let _ = lulu_publish(
        "psu/channel-1",
        "current",
        LogLevel::Info,
        Data::Float32(0.45),
    );
    println!("      ↳ psu/channel-1/current = 0.45 (float32)");
    std::thread::sleep(Duration::from_millis(10));

    let _ = lulu_publish(
        "psu/channel-1",
        "current",
        LogLevel::Warn,
        Data::Float32(0.98),
    );
    println!("      ↳ psu/channel-1/current = 0.98 (float32) [warn]");
    std::thread::sleep(Duration::from_millis(10));

    let _ = lulu_publish(
        "psu/channel-1",
        "current",
        LogLevel::Error,
        Data::Float32(1.05),
    );
    println!("      ↳ psu/channel-1/current = 1.05 (float32) [error]");
    std::thread::sleep(Duration::from_millis(10));

    print_scenario_step("END", "test", "scenario", &format!("{} → FAIL", name2));
    let _ = scenario2.end(
        false,
        Some("Current reached 1.05A, protection did not trigger within 100ms"),
    );
    std::thread::sleep(Duration::from_millis(300));

    // ── Scenario 3: in-progress (no end) ──────────────────────────────────
    let name3 = "signal-integrity-check";

    print_scenario_step("BEG", "test", "scenario", name3);
    let _scenario3 = lulu_scenario(name3).unwrap();
    std::thread::sleep(Duration::from_millis(10));

    let _ = lulu_publish(
        "oscilloscope/probe-a",
        "frequency",
        LogLevel::Info,
        Data::Float64(1_000_000.0),
    );
    println!("      ↳ oscilloscope/probe-a/frequency = 1MHz (float64)");
    std::thread::sleep(Duration::from_millis(10));

    println!("      ↳ (scenario left open — in progress)");
    println!("────────────────────────────────────────────────────────");

    // ── Tool call spans ───────────────────────────────────────────────────
    let tool_source = "agent/copilot";
    let tool_attr = "tool-call";
    let tool_id_ok = "tool-call-read-file-001";
    let tool_metadata_ok = json!({
        "tool_name": "read_file",
        "agent_name": "Copilot",
        "arguments": {"path": "README.md"}
    });

    print_scenario_step("BEG", tool_source, tool_attr, "tool call read_file");
    let _ = lulu_tool_call_beg(
        tool_source,
        tool_attr,
        tool_id_ok,
        "read_file",
        Some(&tool_metadata_ok),
    );
    std::thread::sleep(Duration::from_millis(10));

    let tool_result_ok = json!({"status": "ok", "bytes_read": 2048});
    print_scenario_step(
        "END",
        tool_source,
        tool_attr,
        "tool call read_file → SUCCESS",
    );
    let _ = lulu_tool_call_end(
        tool_source,
        tool_attr,
        tool_id_ok,
        "read_file",
        true,
        None,
        Some(12),
        Some(&tool_metadata_ok),
        Some(&tool_result_ok),
    );
    std::thread::sleep(Duration::from_millis(300));

    let tool_id_fail = "tool-call-run-command-002";
    let tool_metadata_fail = json!({
        "tool_name": "run_in_terminal",
        "agent_name": "Copilot",
        "arguments": {"command": "cargo test"}
    });

    print_scenario_step("BEG", tool_source, tool_attr, "tool call run_in_terminal");
    let _ = lulu_tool_call_beg(
        tool_source,
        tool_attr,
        tool_id_fail,
        "run_in_terminal",
        Some(&tool_metadata_fail),
    );
    std::thread::sleep(Duration::from_millis(10));

    let tool_result_fail = json!({"exit_code": 101, "stderr_lines": 7});
    print_scenario_step(
        "END",
        tool_source,
        tool_attr,
        "tool call run_in_terminal → FAIL",
    );
    let _ = lulu_tool_call_end(
        tool_source,
        tool_attr,
        tool_id_fail,
        "run_in_terminal",
        false,
        Some("cargo test failed with compilation errors"),
        Some(127),
        Some(&tool_metadata_fail),
        Some(&tool_result_fail),
    );
    std::thread::sleep(Duration::from_millis(300));

    // ── Step spans ────────────────────────────────────────────────────────
    let step_scenario = lulu_scenario("step-demo").unwrap();

    // Step 1: passing step
    let step_name_ok = "measure-voltage";
    let step_metadata_ok = json!({"channel": 1, "expected_v": 3.3});

    print_scenario_step("BEG", "test", "scenario", &format!("step {}", step_name_ok));
    let step_ok = step_scenario
        .step_with_metadata(step_name_ok, Some(&step_metadata_ok))
        .unwrap();
    std::thread::sleep(Duration::from_millis(10));

    let step_result_ok = json!({"measured_v": 3.31});
    print_scenario_step(
        "END",
        "test",
        "scenario",
        &format!("step {} → SUCCESS", step_name_ok),
    );
    let _ = step_ok.end(
        true,
        None,
        Some(5),
        Some(&step_metadata_ok),
        Some(&step_result_ok),
    );
    std::thread::sleep(Duration::from_millis(300));

    // Step 2: failing step
    let step_name_fail = "check-ripple";
    let step_metadata_fail = json!({"channel": 1, "max_ripple_mv": 50});

    print_scenario_step(
        "BEG",
        "test",
        "scenario",
        &format!("step {}", step_name_fail),
    );
    let step_fail = step_scenario
        .step_with_metadata(step_name_fail, Some(&step_metadata_fail))
        .unwrap();
    std::thread::sleep(Duration::from_millis(10));

    let step_result_fail = json!({"measured_ripple_mv": 72});
    print_scenario_step(
        "END",
        "test",
        "scenario",
        &format!("step {} → FAIL", step_name_fail),
    );
    let _ = step_fail.end(
        false,
        Some("Ripple 72mV exceeds 50mV limit"),
        Some(8),
        Some(&step_metadata_fail),
        Some(&step_result_fail),
    );

    let _ = step_scenario.end(false, Some("check-ripple failed"));
    println!("────────────────────────────────────────────────────────");
}

fn print_scenario_step(tag: &str, source: &str, attr: &str, info: &str) {
    println!("  [{tag}] {source}/{attr} — {info}");
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let args = parse_args();

    println!("lulu-inject — MQTT test log injector");
    println!("  broker : {}:{}", args.broker_host, args.broker_port);
    println!();

    // ── Step 1: initialise the lulu-logs-client library ───────────────────────────
    //
    // lulu_init() must be called exactly once per process.
    // It spawns a dedicated background Tokio runtime and establishes the MQTT
    // connection. The call blocks until the internal state is ready but does
    // NOT wait for the MQTT handshake to complete.
    let config = LuluConfig {
        broker_host: args.broker_host,
        broker_port: args.broker_port,
        // Prefix used to build the MQTT client ID (a random suffix is appended
        // automatically to avoid collisions when running multiple instances).
        client_id_prefix: "lulu-inject".to_string(),
        // How many messages may be queued while the broker is unreachable.
        queue_capacity: 256,
        // MQTT keep-alive interval in seconds.
        keep_alive_secs: 5,
        // Enable coloured test-scenario output on the terminal.
        terminal_logger: true,
    };

    if let Err(e) = lulu_init(config) {
        eprintln!("[ERROR] lulu_init failed: {e}");
        std::process::exit(1);
    }

    println!("[init] lulu-logs-client initialised");

    // Give the MQTT connection a moment to complete its handshake.
    // In production code you can poll lulu_is_connected() in a retry loop
    // instead of using a blind sleep.
    std::thread::sleep(Duration::from_millis(500));

    if lulu_is_connected() {
        println!("[init] MQTT broker connected");
    } else {
        println!("[init] MQTT broker not yet connected — messages will be queued and flushed once connected");
    }

    println!();

    // ── Step 2: publish all test scenarios ───────────────────────────────────
    //
    // lulu_publish() is non-blocking: it validates the arguments and places a
    // PendingMessage on an internal async channel. A background task picks it
    // up, serialises it as a FlatBuffers LogEntry, and publishes it over MQTT.
    //
    // Topic format:  lulu/{source_seg1}/{source_seg2}/.../{attribute}
    // Source rules:  each path segment must match [a-z0-9-]+ (no uppercase,
    //                no spaces, no slashes at start/end).
    // Attribute:     must be non-empty and must not contain '/'.
    // data:          Data enum variant carrying the native value —
    //                e.g. Data::Float32(3.3), Data::String("ok".into()), Data::Bool(true).

    let scenarios = build_scenarios();
    let total = scenarios.len();

    for (i, s) in scenarios.into_iter().enumerate() {
        let idx = i + 1;

        let data_type = s.data.data_type();
        match lulu_publish(s.source, s.attribute, s.level, s.data) {
            Ok(()) => {
                println!(
                    "[{:>2}/{}] OK   {}/{} — {} ({})",
                    idx, total, s.source, s.attribute, s.description, data_type
                );
            }
            Err(e) => {
                eprintln!(
                    "[{:>2}/{}] ERR  {}/{} — {e}",
                    idx, total, s.source, s.attribute
                );
            }
        }

        // Pace the injection so the UI renders messages progressively.
        std::thread::sleep(Duration::from_millis(10));
    }

    println!();

    // ── Step 2b: inject spans (generic, scenario, tool-call) ──────────────────
    //
    // Uses lulu_span_*(), lulu_scenario_*(), and lulu_tool_call_*() convenience
    // helpers to demonstrate the lulu-logs v1.3.0 span lifecycle types.
    inject_test_scenarios();

    println!();

    // ── Step 2c: heartbeat demo (lulu-logs v1.2.0 §7) ─────────────────────────
    //
    // lulu_start_pulse() spawns a background task that publishes a JSON payload
    // to `lulu-pulse/{source}` every 2 seconds. The first pulse is immediate.
    // Each registered source appears as a live "online" indicator in the UI.
    //
    // Here we register three sources, let them pulse for 6 seconds, then stop
    // two of them so the UI transitions those to "offline".
    println!("[pulse] starting heartbeats for 3 sources…");
    lulu_start_pulse("psu/channel-1", Some("1.0.0")).ok();
    lulu_start_pulse("psu/channel-2", Some("1.0.0")).ok();
    lulu_start_pulse("mcp/filesystem", None).ok();

    // Wait long enough for multiple pulse cycles to be observed by the viewer.
    std::thread::sleep(Duration::from_secs(6));

    // Stop two sources so the viewer shows them as offline after 6 s.
    println!("[pulse] stopping psu/channel-2 and mcp/filesystem");
    lulu_stop_pulse("psu/channel-2");
    lulu_stop_pulse("mcp/filesystem");
    // psu/channel-1 keeps pulsing until lulu_shutdown() cleans up all handles.

    println!();

    // ── Step 3: display stats ─────────────────────────────────────────────────
    //
    // lulu_stats() returns counters maintained by the background send-loop.
    if let Some(stats) = lulu_stats() {
        println!("─── Stats ───────────────────────────────────");
        println!("  published : {}", stats.messages_published);
        println!("  dropped   : {}", stats.messages_dropped);
        println!("  queued    : {} pending", stats.queue_current_size);
        println!("  reconnect : {} times", stats.reconnections);
        println!("─────────────────────────────────────────────");
        println!();
    }

    // ── Step 4: shutdown ──────────────────────────────────────────────────────
    //
    // lulu_shutdown() drains the outgoing queue (up to 5 s timeout), then
    // tears down the MQTT connection and stops the background runtime.
    // Call this before the process exits to avoid losing enqueued messages.
    println!("[shutdown] draining queue…");
    lulu_shutdown();
    println!("[shutdown] done");
}
