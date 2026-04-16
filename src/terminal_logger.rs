//! Minimal terminal logger for test-scenario lifecycle events.
//!
//! When enabled via [`LuluClientConfig::terminal_logger`], coloured one-liners
//! are printed to **stdout** so a developer can follow the test execution at a
//! glance without reading the full MQTT log stream.
//!
//! * `▶ scenario-name` — test started (default colour)
//! * `✓ scenario-name` — test passed  (green)
//! * `✗ scenario-name — error …` — test failed (red)
//! * `  ▸ step-name` — step started (cyan)
//! * `  ✓ step-name` — step passed  (green)
//! * `  ✗ step-name — error …` — step failed (red)

use std::sync::atomic::{AtomicBool, Ordering};

use crate::models::{Data, LogLevel};

/// Global flag — toggled once by [`crate::lulu_init`].
static ENABLED: AtomicBool = AtomicBool::new(false);

// ANSI escape sequences
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";
const RESET: &str = "\x1b[0m";

const ORANGE: &str = "\x1b[38;5;208m";
const BG_GREEN: &str = "\x1b[42;30m";
const BG_RED: &str = "\x1b[41;97m";
const BG_CYAN: &str = "\x1b[46;30m";

/// Enable or disable the terminal logger.  Called from [`crate::lulu_init`].
pub(crate) fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Returns `true` if terminal logging is currently active.
pub(crate) fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Print the start of a test scenario.
pub(crate) fn print_beg(scenario_name: &str) {
    if !is_enabled() {
        return;
    }
    println!("---------------------------------------------------");
    println!("{BG_CYAN} ▶ {scenario_name} {RESET}");
}

/// Print the end of a test scenario with coloured status.
pub(crate) fn print_end(scenario_name: &str, success: bool, error: Option<&str>) {
    if !is_enabled() {
        return;
    }
    if success {
        println!("{BG_GREEN} ✓ {scenario_name} {RESET}");
    } else {
        let err_msg = error.unwrap_or("unknown error");
        println!("{BG_RED} ✗ {scenario_name} — {err_msg} {RESET}");
    }
}

/// Print the start of a test step (indented, cyan).
pub(crate) fn print_step_beg(step_name: &str) {
    if !is_enabled() {
        return;
    }
    println!("    {CYAN}▸ {step_name}{RESET}");
}

/// Print the end of a test step with coloured status (indented).
pub(crate) fn print_step_end(step_name: &str, success: bool, error: Option<&str>) {
    if !is_enabled() {
        return;
    }
    if success {
        println!("    {GREEN}✓ {step_name}{RESET}");
    } else {
        let err_msg = error.unwrap_or("unknown error");
        println!("    {RED}✗ {step_name} — {err_msg}{RESET}");
    }
}

/// Print the start of a generic span.
pub(crate) fn print_span_beg(name: &str) {
    if !is_enabled() {
        return;
    }
    println!("{ORANGE}◆{RESET} {name}");
}

/// Print the end of a generic span with coloured icon only.
pub(crate) fn print_span_end(name: &str, success: bool, error: Option<&str>) {
    if !is_enabled() {
        return;
    }
    if success {
        println!("{ORANGE}◇{RESET} {name}");
    } else {
        let err_msg = error.unwrap_or("unknown error");
        println!("{RED}✗{RESET} {name} — {err_msg}");
    }
}

/// Print a publisher data entry.
///
/// * **String data** — prints only the string content (no source/attribute).
/// * **Other types** — prints `source · attribute = value`.
pub(crate) fn print_publish(source: &str, attribute: &str, _level: &LogLevel, data: &Data) {
    match data {
        Data::String(s) => {
            println!("{s}");
        }
        _ => {
            println!("{source} · {attribute} = {:?}", data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enabled_flag_default_off() {
        // Reset to default for this test
        ENABLED.store(false, Ordering::Relaxed);
        assert!(!is_enabled());
    }

    #[test]
    fn test_set_enabled_on() {
        set_enabled(true);
        assert!(is_enabled());
        // Cleanup
        set_enabled(false);
    }

    #[test]
    fn test_print_beg_does_not_panic_when_disabled() {
        set_enabled(false);
        // Should be a no-op, must not panic
        print_beg("my-scenario");
    }

    #[test]
    fn test_print_end_does_not_panic_when_disabled() {
        set_enabled(false);
        print_end("my-scenario", true, None);
        print_end("my-scenario", false, Some("oops"));
    }

    #[test]
    fn test_print_beg_when_enabled() {
        set_enabled(true);
        // Should not panic; output goes to stdout
        print_beg("voltage-regulation");
        set_enabled(false);
    }

    #[test]
    fn test_print_end_success_when_enabled() {
        set_enabled(true);
        print_end("voltage-regulation", true, None);
        set_enabled(false);
    }

    #[test]
    fn test_print_end_failure_when_enabled() {
        set_enabled(true);
        print_end(
            "voltage-regulation",
            false,
            Some("measured 4.87V, expected 5.00V"),
        );
        set_enabled(false);
    }

    #[test]
    fn test_print_end_failure_default_error() {
        set_enabled(true);
        // error = None → should show "unknown error"
        print_end("voltage-regulation", false, None);
        set_enabled(false);
    }

    #[test]
    fn test_print_step_beg_does_not_panic_when_disabled() {
        set_enabled(false);
        print_step_beg("my-step");
    }

    #[test]
    fn test_print_step_end_does_not_panic_when_disabled() {
        set_enabled(false);
        print_step_end("my-step", true, None);
        print_step_end("my-step", false, Some("oops"));
    }

    #[test]
    fn test_print_step_beg_when_enabled() {
        set_enabled(true);
        print_step_beg("check-voltage");
        set_enabled(false);
    }

    #[test]
    fn test_print_step_end_success_when_enabled() {
        set_enabled(true);
        print_step_end("check-voltage", true, None);
        set_enabled(false);
    }

    #[test]
    fn test_print_step_end_failure_when_enabled() {
        set_enabled(true);
        print_step_end(
            "check-voltage",
            false,
            Some("measured 4.87V, expected 5.00V"),
        );
        set_enabled(false);
    }

    #[test]
    fn test_print_step_end_failure_default_error() {
        set_enabled(true);
        print_step_end("check-voltage", false, None);
        set_enabled(false);
    }
}
