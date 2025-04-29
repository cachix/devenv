use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Convert a SystemTime to Unix seconds.
///
/// Returns an i64 because SQLite doesn't support u64.
/// Values larger than i64::MAX are clamped to i64::MAX.
pub fn system_time_to_unix_seconds(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

/// Convert an integer unix timestamp in seconds to a SystemTime.
///
/// Takes an i64 because SQLite doesn't support u64.
pub fn system_time_from_unix_seconds(seconds: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds.max(0) as u64)
}

/// Get the current system time as Unix seconds.
pub fn now_as_unix_seconds() -> i64 {
    system_time_to_unix_seconds(SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_conversion_roundtrip() {
        let now = SystemTime::now();
        let seconds = system_time_to_unix_seconds(now);
        let roundtrip = system_time_from_unix_seconds(seconds);

        // Compare durations since UNIX_EPOCH to handle rounding issues
        let original_duration = now.duration_since(UNIX_EPOCH).unwrap();
        let roundtrip_duration = roundtrip.duration_since(UNIX_EPOCH).unwrap();

        // They should be within 1 second of each other (due to second-level precision)
        assert!(
            original_duration.as_secs() == roundtrip_duration.as_secs(),
            "Time conversion roundtrip failed: original: {:?}, roundtrip: {:?}",
            original_duration,
            roundtrip_duration
        );
    }

    #[test]
    fn test_negative_seconds_handled() {
        // Test with negative seconds (invalid but could happen with bad data)
        let time = system_time_from_unix_seconds(-1);
        // Should be clamped to UNIX_EPOCH
        assert_eq!(time, UNIX_EPOCH);
    }
}
