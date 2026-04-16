# lulu-logs-client

Rust client library for the [lulu-logs](https://github.com/XdoctorwhoZ/lulu-logs) protocol.

This crate provides a singleton API to send structured log entries over MQTT, serialised as FlatBuffers payloads, to the Lulu-Logs system.

## Features

- **Singleton API** — `lulu_init` / `lulu_publish` / `lulu_shutdown` for simple integration
- **Pre-validated publisher** — `LuluPublisher` validates source and attribute once at construction time
- **Test scenarios & steps** — `lulu_scenario` / `ScenarioHandle::step` for structured test reporting
- **Generic spans** — `lulu_span` builder for arbitrary span instrumentation
- **Heartbeat pulses** — `lulu_start_pulse` / `lulu_stop_pulse` for liveness monitoring
- **Embedded recorder** — `lulu_start_recorder` / `lulu_stop_recorder` for CI / offline recording without the desktop application
- **Terminal logger** — optional coloured terminal output for test scenario lifecycle events

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
lulu-logs-client = { git = "https://github.com/XdoctorwhoZ/lulu-logs-rust" }
```

### Basic example

```rust
use lulu_logs_client::{lulu_init, lulu_publish, lulu_shutdown, LogLevel, Data, LuluConfig};

// Initialize the client
let config = LuluConfig {
    broker_host: "127.0.0.1".to_string(),
    broker_port: 1883,
    ..Default::default()
};
lulu_init(config).unwrap();

// Publish a log entry
lulu_publish(
    "device/sensor-1",           // source (hierarchical path)
    "temperature",                // attribute name
    LogLevel::Info,                // log level
    Data::Float32(23.5),           // data value
).unwrap();

// Gracefully shutdown when done
lulu_shutdown();
```

### Pre-validated publisher

```rust
use lulu_logs_client::{LuluPublisher, Data};

let voltage = LuluPublisher::new("psu/channel-1", "voltage")
    .unwrap()
    .terminal(true);

voltage.info(Data::Float32(3.31)).unwrap();
voltage.warn(Data::Float32(2.80)).unwrap();
```

### Embedded recorder (CI / offline usage)

When you need to record logs without the Lulu-Logs desktop application (e.g. in a CI pipeline), use the embedded recorder. It starts a local MQTT broker, captures every `lulu/#` message published by the current process, and saves them to a `.lulu` file that can be opened later in the application for analysis.

```rust
use lulu_logs_client::{lulu_start_recorder, lulu_stop_recorder, lulu_publish, LogLevel, Data};

// Start the recorder — also calls lulu_init internally.
// Pass None to use the default file name "lulu_recording.lulu" in the current directory.
lulu_start_recorder(Some("my_test_run.lulu".into())).unwrap();

// Publish logs as usual
lulu_publish("device/sensor-1", "temperature", LogLevel::Info, Data::Float32(23.5)).unwrap();

// Stop the recorder: drains the queue, then writes (or appends to) the .lulu file.
lulu_stop_recorder().unwrap();
```

If `my_test_run.lulu` already exists, the new records are **appended** to the existing ones — the file is never overwritten. This makes it safe to call the recorder across multiple CI runs while keeping a single accumulated log file.

### Terminal logger

The terminal logger prints coloured one-liners to **stdout** so you can follow the test execution at a glance without reading the full MQTT log stream.

#### Activating for scenarios, steps & spans

Set `terminal_logger: true` in `LuluConfig` when you initialise the client:

```rust
use lulu_logs_client::{lulu_init, LuluConfig};

lulu_init(LuluConfig {
    terminal_logger: true,
    ..Default::default()
}).unwrap();
```

Once enabled, every call to `lulu_scenario`, `ScenarioHandle::step`, and `lulu_span` will print status lines automatically:

```text
---------------------------------------------------
 ▶ my-scenario              — scenario started
    ▸ check-voltage          — step started   (cyan)
    ✓ check-voltage          — step passed    (green)
 ✓ my-scenario              — scenario passed (green)
 ✗ my-scenario — error …    — scenario failed (red)
```

#### Activating for a publisher

Each `LuluPublisher` has its own terminal toggle, independent of the global flag. Chain `.terminal(true)` after construction:

```rust
use lulu_logs_client::{LuluPublisher, Data};

let voltage = LuluPublisher::new("psu/channel-1", "voltage")
    .unwrap()
    .terminal(true);

voltage.info(Data::Float32(3.31)).unwrap();
voltage.warn(Data::Float32(2.80)).unwrap();
```

This produces output like:

```text
psu/channel-1 · voltage = Float32(3.31)
psu/channel-1 · voltage = Float32(2.80)
```

#### Demo binary

Run the included `lulu-terminal` binary to see the terminal logger in action:

```bash
cargo run --bin lulu-terminal
```

## Included binaries

- **lulu-inject** — Test log injector for UI testing and demonstration
- **lulu-terminal** — Span, scenario & step terminal rendering demo

```bash
cargo run --bin lulu-inject
cargo run --bin lulu-terminal
```

## License

This project is licensed under the Apache License 2.0 — see the [LICENSE](LICENSE) file for details.
