use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Envelope<'a> {
    pub score: f64,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    #[serde(borrow)]
    pub payload: &'a serde_json::value::RawValue,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let json =
            r#"{"score":0.87,"metadata":{"model":"v2"},"payload":{"user_id":42,"data":[1,2,3]}}"#;
        let env: Envelope = serde_json::from_str(json).unwrap();

        assert!((env.score - 0.87).abs() < f64::EPSILON);
        assert_eq!(env.metadata["model"], "v2");
        assert_eq!(env.payload.get(), r#"{"user_id":42,"data":[1,2,3]}"#);

        let serialized = serde_json::to_string(&env).unwrap();
        let re_parsed: Envelope = serde_json::from_str(&serialized).unwrap();
        assert!((re_parsed.score - 0.87).abs() < f64::EPSILON);
    }

    #[test]
    fn minimal_message() {
        let json = r#"{"score":0.5,"payload":null}"#;
        let env: Envelope = serde_json::from_str(json).unwrap();
        assert!((env.score - 0.5).abs() < f64::EPSILON);
        assert!(env.metadata.is_empty());
    }

    #[test]
    fn payload_is_opaque() {
        let json = r#"{"score":0.1,"payload":"just a string"}"#;
        let env: Envelope = serde_json::from_str(json).unwrap();
        assert_eq!(env.payload.get(), r#""just a string""#);
    }
}
