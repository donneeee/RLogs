use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{read_json_with_limit, write_json_atomic_with_limit};

const PARSER_HEALTH_SCHEMA_VERSION: u16 = 1;
const MAXIMUM_PARSER_HEALTH_BYTES: u64 = 128 * 1024;
const MAXIMUM_RETAINED_SESSIONS: usize = 32;
const MAXIMUM_SESSION_ID_BYTES: usize = 256;
const MAXIMUM_VERSION_BYTES: usize = 128;
const MAXIMUM_DETAIL_CHARS: usize = 400;
const CHECKPOINT_INTERVAL_MILLIS: u64 = 15_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParserHealthOutcome {
    Active,
    Complete,
    Failed,
    Interrupted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserHealthSession {
    pub session_id: String,
    pub application_version: String,
    pub client_build: String,
    pub started_unix_millis: u64,
    pub completed_unix_millis: Option<u64>,
    pub outcome: ParserHealthOutcome,
    pub monitored_frame_count: u64,
    pub decoded_event_count: u64,
    pub sealed_run_count: u64,
    pub recoverable_error_count: u64,
    pub capture_queue_saturation_count: u64,
    pub last_progress_unix_millis: Option<u64>,
    pub last_recoverable_error: Option<String>,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserHealthHistory {
    pub schema_version: u16,
    pub sessions: Vec<ParserHealthSession>,
}

impl Default for ParserHealthHistory {
    fn default() -> Self {
        Self {
            schema_version: PARSER_HEALTH_SCHEMA_VERSION,
            sessions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ParserHealthObservation {
    pub monitored_frame_count: u64,
    pub decoded_event_count: u64,
    pub sealed_run_count: u64,
    pub recoverable_error_count: u64,
    pub capture_queue_saturation_count: u64,
    pub last_progress_unix_millis: Option<u64>,
    pub last_recoverable_error: Option<String>,
    pub detail: String,
}

#[derive(Debug)]
pub struct ParserHealthStore {
    path: PathBuf,
    history: ParserHealthHistory,
    last_checkpoint_unix_millis: u64,
}

impl ParserHealthStore {
    pub fn open(path: PathBuf, now_unix_millis: u64) -> Result<Self, String> {
        let parent = path
            .parent()
            .ok_or_else(|| "parser health history path has no parent".to_owned())?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create parser health history folder: {error}"))?;
        let mut history = match std::fs::metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file() {
                    return Err("parser health history is not a regular file".into());
                }
                read_json_with_limit(&path, MAXIMUM_PARSER_HEALTH_BYTES, "parser health history")?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ParserHealthHistory::default()
            }
            Err(error) => {
                return Err(format!("could not inspect parser health history: {error}"));
            }
        };
        validate_history(&history)?;

        let mut recovered_interruption = false;
        for session in &mut history.sessions {
            if session.outcome == ParserHealthOutcome::Active {
                session.outcome = ParserHealthOutcome::Interrupted;
                session.completed_unix_millis = Some(
                    session
                        .last_progress_unix_millis
                        .unwrap_or(now_unix_millis)
                        .min(now_unix_millis),
                );
                session.detail = "rLogs stopped before this parser session recorded a terminal state. The last durable checkpoint is retained for diagnosis.".into();
                recovered_interruption = true;
            }
        }

        let store = Self {
            path,
            history,
            last_checkpoint_unix_millis: now_unix_millis,
        };
        if recovered_interruption {
            store.persist()?;
        }
        Ok(store)
    }

    pub fn empty(path: PathBuf, now_unix_millis: u64) -> Self {
        Self {
            path,
            history: ParserHealthHistory::default(),
            last_checkpoint_unix_millis: now_unix_millis,
        }
    }

    pub fn snapshot(&self) -> ParserHealthHistory {
        self.history.clone()
    }

    pub fn begin(
        &mut self,
        session_id: &str,
        application_version: &str,
        client_build: &str,
        started_unix_millis: u64,
        observation: ParserHealthObservation,
    ) -> Result<(), String> {
        validate_identity(session_id, application_version, client_build)?;
        self.history
            .sessions
            .retain(|session| session.session_id != session_id);
        self.history.sessions.insert(
            0,
            ParserHealthSession {
                session_id: session_id.to_owned(),
                application_version: application_version.to_owned(),
                client_build: client_build.to_owned(),
                started_unix_millis,
                completed_unix_millis: None,
                outcome: ParserHealthOutcome::Active,
                monitored_frame_count: observation.monitored_frame_count,
                decoded_event_count: observation.decoded_event_count,
                sealed_run_count: observation.sealed_run_count,
                recoverable_error_count: observation.recoverable_error_count,
                capture_queue_saturation_count: observation.capture_queue_saturation_count,
                last_progress_unix_millis: observation.last_progress_unix_millis,
                last_recoverable_error: observation.last_recoverable_error,
                detail: bounded_detail(&observation.detail),
            },
        );
        self.history.sessions.truncate(MAXIMUM_RETAINED_SESSIONS);
        self.persist()?;
        self.last_checkpoint_unix_millis = started_unix_millis;
        Ok(())
    }

    pub fn checkpoint(
        &mut self,
        session_id: &str,
        now_unix_millis: u64,
        observation: ParserHealthObservation,
        force: bool,
    ) -> Result<(), String> {
        let session = self
            .history
            .sessions
            .iter_mut()
            .find(|session| session.session_id == session_id)
            .ok_or_else(|| format!("parser health session {session_id} is not registered"))?;
        apply_observation(session, observation);
        if force
            || now_unix_millis.saturating_sub(self.last_checkpoint_unix_millis)
                >= CHECKPOINT_INTERVAL_MILLIS
        {
            self.persist()?;
            self.last_checkpoint_unix_millis = now_unix_millis;
        }
        Ok(())
    }

    pub fn finish(
        &mut self,
        session_id: &str,
        outcome: ParserHealthOutcome,
        completed_unix_millis: u64,
        observation: ParserHealthObservation,
    ) -> Result<(), String> {
        if outcome == ParserHealthOutcome::Active {
            return Err("a finished parser health session cannot remain active".into());
        }
        let session = self
            .history
            .sessions
            .iter_mut()
            .find(|session| session.session_id == session_id)
            .ok_or_else(|| format!("parser health session {session_id} is not registered"))?;
        apply_observation(session, observation);
        session.outcome = outcome;
        session.completed_unix_millis = Some(completed_unix_millis);
        self.persist()?;
        self.last_checkpoint_unix_millis = completed_unix_millis;
        Ok(())
    }

    fn persist(&self) -> Result<(), String> {
        validate_history(&self.history)?;
        write_json_atomic_with_limit(
            &self.path,
            &self.history,
            MAXIMUM_PARSER_HEALTH_BYTES,
            "parser health history",
        )
    }
}

fn apply_observation(session: &mut ParserHealthSession, observation: ParserHealthObservation) {
    session.monitored_frame_count = observation.monitored_frame_count;
    session.decoded_event_count = observation.decoded_event_count;
    session.sealed_run_count = observation.sealed_run_count;
    session.recoverable_error_count = observation.recoverable_error_count;
    session.capture_queue_saturation_count = observation.capture_queue_saturation_count;
    session.last_progress_unix_millis = observation.last_progress_unix_millis;
    session.last_recoverable_error = observation.last_recoverable_error;
    session.detail = bounded_detail(&observation.detail);
}

fn validate_history(history: &ParserHealthHistory) -> Result<(), String> {
    if history.schema_version != PARSER_HEALTH_SCHEMA_VERSION {
        return Err(format!(
            "unsupported parser health history schema {}",
            history.schema_version
        ));
    }
    if history.sessions.len() > MAXIMUM_RETAINED_SESSIONS {
        return Err(format!(
            "parser health history exceeds {MAXIMUM_RETAINED_SESSIONS} sessions"
        ));
    }
    for session in &history.sessions {
        validate_identity(
            &session.session_id,
            &session.application_version,
            &session.client_build,
        )?;
        if session.started_unix_millis == 0 {
            return Err("parser health session has an invalid start time".into());
        }
        if session
            .completed_unix_millis
            .is_some_and(|completed| completed < session.started_unix_millis)
        {
            return Err("parser health session ends before it starts".into());
        }
        if session.detail.chars().count() > MAXIMUM_DETAIL_CHARS
            || session
                .last_recoverable_error
                .as_ref()
                .is_some_and(|detail| detail.chars().count() > MAXIMUM_DETAIL_CHARS)
        {
            return Err("parser health session detail exceeds its bound".into());
        }
    }
    Ok(())
}

fn validate_identity(
    session_id: &str,
    application_version: &str,
    client_build: &str,
) -> Result<(), String> {
    if session_id.trim().is_empty() || session_id.len() > MAXIMUM_SESSION_ID_BYTES {
        return Err("parser health session ID is invalid".into());
    }
    if application_version.trim().is_empty()
        || application_version.len() > MAXIMUM_VERSION_BYTES
        || client_build.trim().is_empty()
        || client_build.len() > MAXIMUM_VERSION_BYTES
    {
        return Err("parser health version identity is invalid".into());
    }
    Ok(())
}

fn bounded_detail(detail: &str) -> String {
    let normalized = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= MAXIMUM_DETAIL_CHARS {
        return normalized;
    }
    normalized
        .chars()
        .take(MAXIMUM_DETAIL_CHARS.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rlogs-parser-health-{}-{name}.json",
            std::process::id()
        ))
    }

    fn observation(frames: u64) -> ParserHealthObservation {
        ParserHealthObservation {
            monitored_frame_count: frames,
            decoded_event_count: frames.saturating_mul(2),
            sealed_run_count: 1,
            recoverable_error_count: 0,
            capture_queue_saturation_count: 0,
            last_progress_unix_millis: Some(1_000 + frames),
            last_recoverable_error: None,
            detail: format!("healthy at {frames} frames"),
        }
    }

    #[test]
    fn sessions_persist_and_finish_with_exact_counters() {
        let path = test_path("finish");
        let _ = std::fs::remove_file(&path);
        let mut store = ParserHealthStore::open(path.clone(), 1_000).unwrap();
        store
            .begin("monitor-1", "0.1.63", "steam-1", 1_000, observation(0))
            .unwrap();
        store
            .checkpoint("monitor-1", 16_000, observation(50), false)
            .unwrap();
        store
            .finish(
                "monitor-1",
                ParserHealthOutcome::Complete,
                17_000,
                observation(75),
            )
            .unwrap();

        let reopened = ParserHealthStore::open(path.clone(), 20_000).unwrap();
        let session = &reopened.snapshot().sessions[0];
        assert_eq!(session.outcome, ParserHealthOutcome::Complete);
        assert_eq!(session.monitored_frame_count, 75);
        assert_eq!(session.decoded_event_count, 150);
        assert_eq!(session.completed_unix_millis, Some(17_000));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_active_session_becomes_an_interruption_after_restart() {
        let path = test_path("interrupted");
        let _ = std::fs::remove_file(&path);
        let mut store = ParserHealthStore::open(path.clone(), 1_000).unwrap();
        store
            .begin("monitor-2", "0.1.63", "steam-2", 1_000, observation(4))
            .unwrap();
        drop(store);

        let reopened = ParserHealthStore::open(path.clone(), 5_000).unwrap();
        let session = &reopened.snapshot().sessions[0];
        assert_eq!(session.outcome, ParserHealthOutcome::Interrupted);
        assert_eq!(session.completed_unix_millis, Some(1_004));
        assert!(session.detail.contains("stopped before"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn history_is_bounded_to_the_newest_sessions() {
        let path = test_path("bounded");
        let _ = std::fs::remove_file(&path);
        let mut store = ParserHealthStore::open(path.clone(), 1_000).unwrap();
        for index in 0..40_u64 {
            let session_id = format!("monitor-{index}");
            store
                .begin(
                    &session_id,
                    "0.1.63",
                    "steam-3",
                    1_000 + index,
                    observation(index),
                )
                .unwrap();
            store
                .finish(
                    &session_id,
                    ParserHealthOutcome::Complete,
                    2_000 + index,
                    observation(index),
                )
                .unwrap();
        }
        let history = store.snapshot();
        assert_eq!(history.sessions.len(), MAXIMUM_RETAINED_SESSIONS);
        assert_eq!(history.sessions[0].session_id, "monitor-39");
        assert_eq!(history.sessions[31].session_id, "monitor-8");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn oversized_history_is_rejected_without_allocating_it() {
        let path = test_path("oversized");
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, vec![b'x'; MAXIMUM_PARSER_HEALTH_BYTES as usize + 1]).unwrap();
        assert!(ParserHealthStore::open(path.clone(), 1_000).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn helper_keeps_a_nonfatal_empty_store_available() {
        let path = test_path("empty");
        let store = ParserHealthStore::empty(path, 1_000);
        assert!(store.snapshot().sessions.is_empty());
    }

    #[test]
    fn detail_is_normalized_and_bounded() {
        let detail = bounded_detail(&format!("one\n two {}", "x".repeat(500)));
        assert_eq!(detail.chars().count(), MAXIMUM_DETAIL_CHARS);
        assert!(!detail.contains('\n'));
        assert!(detail.ends_with('…'));
    }

    #[test]
    fn path_parent_is_required() {
        assert!(ParserHealthStore::open(PathBuf::new(), 1_000).is_err());
    }
}
