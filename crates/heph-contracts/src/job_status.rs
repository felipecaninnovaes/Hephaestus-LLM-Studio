use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Preparing,
    Dispatched,
    Running,
    Done,
    Failed,
    Cancelled,
    Cancelling,
    #[serde(other)]
    Unknown,
}

impl JobStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Preparing => "preparing",
            Self::Dispatched => "dispatched",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Cancelling => "cancelling",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for JobStatus {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_terminal() {
        assert!(JobStatus::Done.is_terminal());
        assert!(JobStatus::Failed.is_terminal());
        assert!(JobStatus::Cancelled.is_terminal());

        assert!(!JobStatus::Queued.is_terminal());
        assert!(!JobStatus::Preparing.is_terminal());
        assert!(!JobStatus::Dispatched.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
        assert!(!JobStatus::Cancelling.is_terminal());
        assert!(!JobStatus::Unknown.is_terminal());
    }

    #[test]
    fn test_as_str_and_display() {
        assert_eq!(JobStatus::Queued.as_str(), "queued");
        assert_eq!(JobStatus::Preparing.to_string(), "preparing");
        assert_eq!(JobStatus::Dispatched.as_ref(), "dispatched");
        assert_eq!(JobStatus::Running.as_str(), "running");
        assert_eq!(JobStatus::Done.as_str(), "done");
        assert_eq!(JobStatus::Failed.as_str(), "failed");
        assert_eq!(JobStatus::Cancelled.as_str(), "cancelled");
        assert_eq!(JobStatus::Cancelling.as_str(), "cancelling");
        assert_eq!(JobStatus::Unknown.as_str(), "unknown");
    }

    #[test]
    fn test_serde_roundtrip() {
        let serialized = serde_json::to_string(&JobStatus::Running).unwrap();
        assert_eq!(serialized, "\"running\"");

        let deserialized: JobStatus = serde_json::from_str("\"running\"").unwrap();
        assert_eq!(deserialized, JobStatus::Running);

        let unknown: JobStatus = serde_json::from_str("\"something_else\"").unwrap();
        assert_eq!(unknown, JobStatus::Unknown);
    }
}
