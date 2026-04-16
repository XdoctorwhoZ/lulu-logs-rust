use crate::error::LuluError;

/// Valide un segment de topic unique.
/// Règles : non vide, contient uniquement [a-zA-Z0-9_-].
fn validate_segment(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Fractionne source sur '/', valide chaque segment.
/// Retourne `Err(LuluError::InvalidSource)` si invalide.
pub fn parse_source(source: &str) -> Result<Vec<String>, LuluError> {
    let segments: Vec<&str> = source.split('/').collect();
    if segments.is_empty() {
        return Err(LuluError::InvalidSource(source.to_string()));
    }
    for seg in &segments {
        if !validate_segment(seg) {
            return Err(LuluError::InvalidSource((*seg).to_string()));
        }
    }
    Ok(segments.into_iter().map(String::from).collect())
}

/// Retourne `Err(LuluError::InvalidAttribute)` si vide ou contient '/'.
pub fn validate_attribute(attribute: &str) -> Result<(), LuluError> {
    if attribute.is_empty() || attribute.contains('/') {
        return Err(LuluError::InvalidAttribute(attribute.to_string()));
    }
    Ok(())
}

/// Construit le topic MQTT complet.
/// Format : `"lulu/{segment_1}/.../{segment_n}/{attribute}"`
pub fn build_topic(source_segments: &[String], attribute: &str) -> String {
    format!("lulu/{}/{}", source_segments.join("/"), attribute)
}

/// Construit le topic MQTT heartbeat.
/// Format : `"lulu-pulse/{segment_1}/.../{segment_n}"`
pub fn build_pulse_topic(source_segments: &[String]) -> String {
    format!("lulu-pulse/{}", source_segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_source_valid() {
        let segs = parse_source("psu/power-supply/channel-1").unwrap();
        assert_eq!(segs, vec!["psu", "power-supply", "channel-1"]);
    }

    #[test]
    fn test_parse_source_single() {
        let segs = parse_source("my-service").unwrap();
        assert_eq!(segs, vec!["my-service"]);
    }

    #[test]
    fn test_parse_source_invalid_segment() {
        assert!(parse_source("a/b/").is_err()); // trailing slash → empty segment
    }

    #[test]
    fn test_parse_source_with_uppercase() {
        let segs = parse_source("My-Service").unwrap();
        assert_eq!(segs, vec!["My-Service"]);
    }

    #[test]
    fn test_validate_attribute_ok() {
        assert!(validate_attribute("voltage").is_ok());
        assert!(validate_attribute("read-file").is_ok());
    }

    #[test]
    fn test_validate_attribute_err() {
        assert!(validate_attribute("").is_err());
        assert!(validate_attribute("a/b").is_err());
    }

    #[test]
    fn test_build_topic() {
        let segs = vec!["psu".to_string(), "power-supply".to_string()];
        assert_eq!(build_topic(&segs, "voltage"), "lulu/psu/power-supply/voltage");
    }

    #[test]
    fn test_build_pulse_topic() {
        let segs = vec!["psu".to_string(), "power-supply".to_string(), "channel-1".to_string()];
        assert_eq!(build_pulse_topic(&segs), "lulu-pulse/psu/power-supply/channel-1");
        let single = vec!["my-service".to_string()];
        assert_eq!(build_pulse_topic(&single), "lulu-pulse/my-service");
    }
}
