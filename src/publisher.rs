use crate::models::{Data, LogLevel};
use crate::serializer::PendingMessage;
use crate::terminal_logger;
use crate::topic;
use crate::GLOBAL_CLIENT;

/// A pre-validated publisher bound to a fixed `source` and `attribute`.
///
/// Created via [`LuluPublisher::new`], which validates the source and attribute
/// once at construction time.  The convenience methods (`debug`, `info`, `warn`,
/// `error`, …) only require a [`Data`] payload, avoiding repeated validation on
/// every publish call.
///
/// When `terminal` is `true`, every publish also prints a coloured one-liner to
/// stdout so a developer can follow the data flow at a glance.
///
/// # Example
/// ```no_run
/// use lulu_logs::{LuluPublisher, Data};
///
/// let voltage = LuluPublisher::new("psu/channel-1", "voltage")
///     .terminal(true);
/// voltage.info(Data::Float32(3.31));
/// voltage.warn(Data::Float32(2.80));
/// ```
#[derive(Clone, Debug)]
pub struct LuluPublisher {
    source_segments: Vec<String>,
    source: String,
    attribute: String,
    terminal: bool,
}

impl LuluPublisher {
    /// Creates a new publisher for the given `source` and `attribute`.
    ///
    /// Source and attribute are validated exactly once here.
    /// Terminal output is disabled by default — call [`.terminal(true)`](Self::terminal)
    /// to enable it.
    ///
    /// # Panics
    /// Panics if `source` or `attribute` violate the lulu-logs topic naming rules.
    pub fn new(source: &str, attribute: &str) -> Self {
        let source_segments = topic::parse_source(source)
            .unwrap_or_else(|e| panic!("invalid source {:?}: {}", source, e));
        topic::validate_attribute(attribute)
            .unwrap_or_else(|e| panic!("invalid attribute {:?}: {}", attribute, e));
        Self {
            source_segments,
            source: source.to_string(),
            attribute: attribute.to_string(),
            terminal: false,
        }
    }

    /// Returns a new publisher with a different `attribute`, keeping the same
    /// `source` and `terminal` settings.
    ///
    /// # Example
    /// ```no_run
    /// use lulu_logs::{LuluPublisher, Data};
    ///
    /// let psu = LuluPublisher::new("psu/channel-1", "voltage");
    /// psu.att("current").info(Data::Float32(1.25));
    /// ```
    ///
    /// # Panics
    /// Panics if `attribute` is empty or contains `/`.
    pub fn att(&self, attribute: &str) -> Self {
        topic::validate_attribute(attribute)
            .unwrap_or_else(|e| panic!("invalid attribute {:?}: {}", attribute, e));
        Self {
            source_segments: self.source_segments.clone(),
            source: self.source.clone(),
            attribute: attribute.to_string(),
            terminal: self.terminal,
        }
    }

    /// Creates a publisher from pre-validated parts.  No validation is
    /// performed — callers must guarantee that `source_segments` and
    /// `attribute` are already valid.
    pub(crate) fn from_parts(
        source_segments: Vec<String>,
        source: String,
        attribute: String,
    ) -> Self {
        Self {
            source_segments,
            source,
            attribute,
            terminal: false,
        }
    }

    /// Enables or disables terminal output for this publisher.
    ///
    /// When enabled, each publish prints a coloured line to stdout:
    /// ```text
    /// [INFO ]  psu/channel-1 · voltage = Float32(3.31)
    /// [WARN ]  psu/channel-1 · voltage = Float32(2.80)
    /// [INFO ]  hello world
    /// ```
    pub fn terminal(mut self, enabled: bool) -> Self {
        self.terminal = enabled;
        self
    }

    fn publish(&self, level: LogLevel, data: Data) {
        if self.terminal {
            terminal_logger::print_publish(&self.source, &self.attribute, &level, &data);
        }
        if let Some(client) = GLOBAL_CLIENT.get() {
            let _ = client.publish(PendingMessage {
                source_segments: self.source_segments.clone(),
                attribute: self.attribute.clone(),
                level,
                data,
            });
        }
    }

    /// Publishes a `Trace`-level log entry.
    pub fn trace(&self, data: Data) {
        self.publish(LogLevel::Trace, data)
    }

    /// Publishes a `Debug`-level log entry.
    pub fn debug(&self, data: Data) {
        self.publish(LogLevel::Debug, data)
    }

    /// Publishes an `Info`-level log entry.
    pub fn info(&self, data: Data) {
        self.publish(LogLevel::Info, data)
    }

    /// Publishes a `Warn`-level log entry.
    pub fn warn(&self, data: Data) {
        self.publish(LogLevel::Warn, data)
    }

    /// Publishes an `Error`-level log entry.
    pub fn error(&self, data: Data) {
        self.publish(LogLevel::Error, data)
    }

    /// Publishes a `Fatal`-level log entry.
    pub fn fatal(&self, data: Data) {
        self.publish(LogLevel::Fatal, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_att_changes_attribute() {
        let pub1 = LuluPublisher::new("psu/channel-1", "voltage");
        let pub2 = pub1.att("current");
        assert_eq!(pub2.attribute, "current");
        // source is preserved
        assert_eq!(pub2.source, "psu/channel-1");
        assert_eq!(pub2.source_segments, vec!["psu", "channel-1"]);
    }

    #[test]
    fn test_att_preserves_terminal_flag() {
        let pub1 = LuluPublisher::new("psu/channel-1", "voltage").terminal(true);
        let pub2 = pub1.att("current");
        assert!(pub2.terminal);
    }

    #[test]
    fn test_att_does_not_modify_original() {
        let pub1 = LuluPublisher::new("psu/channel-1", "voltage");
        let _pub2 = pub1.att("current");
        assert_eq!(pub1.attribute, "voltage");
    }

    #[test]
    #[should_panic(expected = "invalid attribute")]
    fn test_att_rejects_empty_attribute() {
        let pub1 = LuluPublisher::new("psu/channel-1", "voltage");
        pub1.att("");
    }

    #[test]
    #[should_panic(expected = "invalid attribute")]
    fn test_att_rejects_attribute_with_slash() {
        let pub1 = LuluPublisher::new("psu/channel-1", "voltage");
        pub1.att("a/b");
    }
}
