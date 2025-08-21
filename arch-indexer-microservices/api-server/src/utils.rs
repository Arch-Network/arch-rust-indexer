use chrono::{DateTime, Utc};

/// Convert Arch node timestamp (microseconds since Unix epoch) to DateTime<Utc>
/// 
/// The Arch node always returns timestamps in microseconds, so we simply divide by 1,000,000
/// to convert to seconds, then create a DateTime from the Unix timestamp.
pub fn convert_arch_timestamp(timestamp_microseconds: i64) -> DateTime<Utc> {
    // Arch node returns timestamps in microseconds since Unix epoch
    let timestamp_seconds = timestamp_microseconds / 1_000_000;
    
    // Convert Unix timestamp (seconds) to DateTime<Utc>
    DateTime::from_timestamp(timestamp_seconds, 0)
        .unwrap_or_else(|| Utc::now())
}
