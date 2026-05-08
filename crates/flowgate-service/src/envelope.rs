use async_nats::HeaderMap;

pub const SCORE_HEADER: &str = "Flowgate-Score";
pub const THRESHOLD_HEADER: &str = "Flowgate-Threshold";
pub const STATE_HEADER: &str = "Flowgate-State";

pub fn extract_score(headers: Option<&HeaderMap>) -> Option<f64> {
    headers?.get(SCORE_HEADER)?.as_str().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_valid_score() {
        let mut headers = HeaderMap::new();
        headers.insert(SCORE_HEADER, "0.87");
        assert!((extract_score(Some(&headers)).unwrap() - 0.87).abs() < f64::EPSILON);
    }

    #[test]
    fn extract_missing_header() {
        let headers = HeaderMap::new();
        assert!(extract_score(Some(&headers)).is_none());
    }

    #[test]
    fn extract_no_headers() {
        assert!(extract_score(None).is_none());
    }

    #[test]
    fn extract_invalid_score() {
        let mut headers = HeaderMap::new();
        headers.insert(SCORE_HEADER, "not_a_number");
        assert!(extract_score(Some(&headers)).is_none());
    }
}
