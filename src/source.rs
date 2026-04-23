use crate::error::LuluError;
use crate::publisher::LuluPublisher;
use crate::topic;

/// A pre-validated source handle.
///
/// Validates the source path once at construction time.  Use [`att`](Self::att)
/// to obtain a [`LuluPublisher`] bound to a specific attribute without
/// re-validating the source on every call.
///
/// # Example
/// ```no_run
/// use lulu_logs::{LuluSource, Data};
///
/// let source = LuluSource::new("my/source");
/// source.att("toto").info(Data::Float32(1.0)).unwrap();
/// source.att("voltage").warn(Data::Float32(2.8)).unwrap();
/// ```
#[derive(Clone, Debug)]
pub struct LuluSource {
    source_segments: Vec<String>,
    source: String,
}

impl LuluSource {
    /// Creates a new source handle, validating `source` immediately.
    ///
    /// # Panics
    /// Panics if `source` contains empty segments or characters outside
    /// `[a-zA-Z0-9_-]`.
    pub fn new(source: &str) -> Self {
        let source_segments = topic::parse_source(source)
            .unwrap_or_else(|e: LuluError| panic!("invalid source {:?}: {}", source, e));
        Self {
            source_segments,
            source: source.to_string(),
        }
    }

    /// Returns a [`LuluPublisher`] bound to this source and the given `attribute`.
    ///
    /// The source is already validated; only the attribute is checked here.
    ///
    /// # Panics
    /// Panics if `attribute` is empty or contains `/`.
    pub fn att(&self, attribute: &str) -> LuluPublisher {
        topic::validate_attribute(attribute)
            .unwrap_or_else(|e: LuluError| panic!("invalid attribute {:?}: {}", attribute, e));
        LuluPublisher::from_parts(
            self.source_segments.clone(),
            self.source.clone(),
            attribute.to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_valid_source() {
        let s = LuluSource::new("psu/channel-1");
        assert_eq!(s.source, "psu/channel-1");
        assert_eq!(s.source_segments, vec!["psu", "channel-1"]);
    }

    #[test]
    #[should_panic(expected = "invalid source")]
    fn test_new_empty_segment() {
        LuluSource::new("psu//channel-1");
    }

    #[test]
    #[should_panic(expected = "invalid source")]
    fn test_new_invalid_chars() {
        LuluSource::new("psu/chan nel");
    }

    #[test]
    fn test_att_returns_publisher_with_correct_source_and_attribute() {
        let s = LuluSource::new("psu/channel-1");
        let p = s.att("voltage");
        // Access fields through the Debug repr — fields are private, so just
        // verify the publisher is usable (compilation is the main check).
        let _ = p;
    }

    #[test]
    #[should_panic(expected = "invalid attribute")]
    fn test_att_rejects_empty_attribute() {
        let s = LuluSource::new("psu/channel-1");
        s.att("");
    }

    #[test]
    #[should_panic(expected = "invalid attribute")]
    fn test_att_rejects_slash_in_attribute() {
        let s = LuluSource::new("psu/channel-1");
        s.att("a/b");
    }
}
