use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

// --- Enums ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewType {
    Code,
    Plan,
    Manuscript,
    Architecture,
    Custom,
}

impl fmt::Display for ReviewType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Code => write!(f, "code"),
            Self::Plan => write!(f, "plan"),
            Self::Manuscript => write!(f, "manuscript"),
            Self::Architecture => write!(f, "architecture"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

impl FromStr for ReviewType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "code" => Ok(Self::Code),
            "plan" => Ok(Self::Plan),
            "manuscript" => Ok(Self::Manuscript),
            "architecture" => Ok(Self::Architecture),
            "custom" => Ok(Self::Custom),
            other => Err(format!("unknown review type: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Completed,
    Abandoned,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Completed => write!(f, "completed"),
            Self::Abandoned => write!(f, "abandoned"),
        }
    }
}

impl FromStr for SessionStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "completed" => Ok(Self::Completed),
            "abandoned" => Ok(Self::Abandoned),
            other => Err(format!("unknown session status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerType {
    Regular,
    Harsh,
    Grounded,
}

impl fmt::Display for ReviewerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Regular => write!(f, "regular"),
            Self::Harsh => write!(f, "harsh"),
            Self::Grounded => write!(f, "grounded"),
        }
    }
}

impl FromStr for ReviewerType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "regular" => Ok(Self::Regular),
            "harsh" => Ok(Self::Harsh),
            "grounded" => Ok(Self::Grounded),
            other => Err(format!("unknown reviewer type: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundOutcome {
    Approved,
    Rejected,
    Conditional,
}

impl fmt::Display for RoundOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Approved => write!(f, "approved"),
            Self::Rejected => write!(f, "rejected"),
            Self::Conditional => write!(f, "conditional"),
        }
    }
}

impl FromStr for RoundOutcome {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "conditional" => Ok(Self::Conditional),
            other => Err(format!("unknown round outcome: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalType {
    Addressed,
    Acknowledged,
    NeedsRevision,
}

impl fmt::Display for SignalType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Addressed => write!(f, "addressed"),
            Self::Acknowledged => write!(f, "acknowledged"),
            Self::NeedsRevision => write!(f, "needs_revision"),
        }
    }
}

impl FromStr for SignalType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "addressed" => Ok(Self::Addressed),
            "acknowledged" => Ok(Self::Acknowledged),
            "needs_revision" => Ok(Self::NeedsRevision),
            other => Err(format!("unknown signal type: {other}")),
        }
    }
}

// --- Structs ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub target_path: String,
    pub review_type: ReviewType,
    pub status: SessionStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Round {
    pub id: i64,
    pub session_id: String,
    pub round_number: i32,
    pub outcome: Option<RoundOutcome>,
    pub outcome_comment: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub id: i64,
    pub round_id: i64,
    pub reviewer_type: ReviewerType,
    pub file_path: String,
    pub content_hash: String,
    pub bytes_written: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub id: i64,
    pub session_id: String,
    pub signal_type: SignalType,
    pub source_label: String,
    pub comment: Option<String>,
    pub created_at: String,
}

// --- Helpers ---

/// Returns the current UTC time as an ISO 8601 string (YYYY-MM-DDTHH:MM:SSZ).
pub fn now_iso8601() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Convert epoch seconds to date/time components
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year/month/day from days since epoch (1970-01-01)
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Algorithm: count years, then months, then remaining days
    let mut year = 1970;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_days: [u64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if days < md {
            month = i as u64 + 1;
            break;
        }
        days -= md;
    }

    (year, month, days + 1)
}

fn is_leap_year(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_review_type_roundtrip() {
        for rt in [
            ReviewType::Code,
            ReviewType::Plan,
            ReviewType::Manuscript,
            ReviewType::Architecture,
            ReviewType::Custom,
        ] {
            let s = rt.to_string();
            let parsed: ReviewType = s.parse().unwrap();
            assert_eq!(rt, parsed);
        }
    }

    #[test]
    fn test_session_status_roundtrip() {
        for ss in [
            SessionStatus::Active,
            SessionStatus::Completed,
            SessionStatus::Abandoned,
        ] {
            let s = ss.to_string();
            let parsed: SessionStatus = s.parse().unwrap();
            assert_eq!(ss, parsed);
        }
    }

    #[test]
    fn test_reviewer_type_roundtrip() {
        for rt in [
            ReviewerType::Regular,
            ReviewerType::Harsh,
            ReviewerType::Grounded,
        ] {
            let s = rt.to_string();
            let parsed: ReviewerType = s.parse().unwrap();
            assert_eq!(rt, parsed);
        }
    }

    #[test]
    fn test_round_outcome_roundtrip() {
        for ro in [
            RoundOutcome::Approved,
            RoundOutcome::Rejected,
            RoundOutcome::Conditional,
        ] {
            let s = ro.to_string();
            let parsed: RoundOutcome = s.parse().unwrap();
            assert_eq!(ro, parsed);
        }
    }

    #[test]
    fn test_signal_type_roundtrip() {
        for st in [
            SignalType::Addressed,
            SignalType::Acknowledged,
            SignalType::NeedsRevision,
        ] {
            let s = st.to_string();
            let parsed: SignalType = s.parse().unwrap();
            assert_eq!(st, parsed);
        }
    }

    #[test]
    fn test_invalid_enum_parse() {
        assert!(ReviewType::from_str("invalid").is_err());
        assert!(SessionStatus::from_str("invalid").is_err());
        assert!(ReviewerType::from_str("invalid").is_err());
        assert!(RoundOutcome::from_str("invalid").is_err());
        assert!(SignalType::from_str("invalid").is_err());
    }

    #[test]
    fn test_now_iso8601_format() {
        let ts = now_iso8601();
        // Should match YYYY-MM-DDTHH:MM:SSZ
        assert_eq!(ts.len(), 20);
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
        assert_eq!(&ts[19..20], "Z");
    }

    #[test]
    fn test_days_to_ymd_epoch() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_known_date() {
        // 2024-01-01 = 19723 days since epoch
        let (y, m, d) = days_to_ymd(19723);
        assert_eq!((y, m, d), (2024, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_leap_year() {
        // 2024-02-29 = 19723 + 31 + 28 = 19782 days (2024 is leap year, so Feb has 29 days)
        let (y, m, d) = days_to_ymd(19723 + 31 + 28);
        assert_eq!((y, m, d), (2024, 2, 29));
    }

    #[test]
    fn test_serde_json_roundtrip() {
        let session = Session {
            id: "test-uuid".to_string(),
            target_path: "/tmp/test.rs".to_string(),
            review_type: ReviewType::Code,
            status: SessionStatus::Active,
            created_at: "2026-04-07T12:00:00Z".to_string(),
            updated_at: "2026-04-07T12:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&session).unwrap();
        let parsed: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, session.id);
        assert_eq!(parsed.review_type, ReviewType::Code);
        assert_eq!(parsed.status, SessionStatus::Active);
    }
}
