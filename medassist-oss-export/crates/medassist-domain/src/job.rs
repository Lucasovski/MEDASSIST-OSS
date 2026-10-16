//! Async job model shared by transcription, OCR and AI inference work.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobState::Pending => "pending",
            JobState::Running => "running",
            JobState::Completed => "completed",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobState::Completed | JobState::Failed | JobState::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Transcription,
    DocumentExtraction,
    AiDocumentationDraft,
}

/// A durable record of background work with explicit state transitions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsyncJob {
    pub id: Uuid,
    pub org_id: Uuid,
    pub kind: JobKind,
    pub state: JobState,
    /// Attempts already made (bounded retries enforced by the worker).
    pub attempts: u32,
    pub max_attempts: u32,
    pub error: Option<String>,
    pub result_ref: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AsyncJob {
    pub fn new(id: Uuid, org_id: Uuid, kind: JobKind, now: DateTime<Utc>) -> Self {
        Self {
            id,
            org_id,
            kind,
            state: JobState::Pending,
            attempts: 0,
            max_attempts: 3,
            error: None,
            result_ref: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn can_retry(&self) -> bool {
        self.attempts < self.max_attempts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_budget() {
        let now = Utc::now();
        let mut j = AsyncJob::new(Uuid::new_v4(), Uuid::new_v4(), JobKind::Transcription, now);
        assert!(j.can_retry());
        j.attempts = j.max_attempts;
        assert!(!j.can_retry());
        assert!(!j.state.is_terminal());
        j.state = JobState::Completed;
        assert!(j.state.is_terminal());
    }
}
