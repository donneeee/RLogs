use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use rlogs_submission::{
    QUEUED_SUBMISSION_SCHEMA_VERSION, QueuedSubmission, ReportVisibility, SubmissionState,
};
use serde::Serialize;

const QUEUE_VIEW_SCHEMA_VERSION: u16 = 1;
const QUEUE_FILE_SUFFIX: &str = ".submission.json";
const MAXIMUM_QUEUE_ENTRIES: usize = 256;
const MAXIMUM_QUEUE_ENTRY_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct LocalSubmissionQueue {
    directory: PathBuf,
    entries: BTreeMap<String, QueuedSubmission>,
    issues: Vec<String>,
}

impl LocalSubmissionQueue {
    pub fn open(directory: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("could not create submission queue directory: {error}"))?;
        let mut queue = Self {
            directory,
            entries: BTreeMap::new(),
            issues: Vec::new(),
        };
        queue.reload()?;
        Ok(queue)
    }

    pub fn reload(&mut self) -> Result<(), String> {
        let directory_entries = std::fs::read_dir(&self.directory)
            .map_err(|error| format!("could not read submission queue directory: {error}"))?;
        let mut candidates = Vec::new();
        let mut issues = Vec::new();
        for directory_entry in directory_entries {
            let entry = match directory_entry {
                Ok(entry) => entry,
                Err(error) => {
                    issues.push(format!(
                        "Could not inspect a submission queue entry: {error}"
                    ));
                    continue;
                }
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    issues.push(format!("{name}: could not inspect queue entry: {error}"));
                    continue;
                }
            };
            if file_type.is_file() && name.ends_with(QUEUE_FILE_SUFFIX) {
                candidates.push((name, entry.path()));
            }
        }
        candidates.sort_by(|left, right| left.0.cmp(&right.0));

        let mut entries = BTreeMap::new();
        if candidates.len() > MAXIMUM_QUEUE_ENTRIES {
            issues.push(format!(
                "Submission queue has {} entries; only the first {} were loaded.",
                candidates.len(),
                MAXIMUM_QUEUE_ENTRIES
            ));
            candidates.truncate(MAXIMUM_QUEUE_ENTRIES);
        }
        for (name, path) in candidates {
            match load_entry(&path, &name) {
                Ok(entry) => {
                    let queue_id = entry.queue_id.to_string();
                    if entries.insert(queue_id.clone(), entry).is_some() {
                        issues.push(format!(
                            "Duplicate submission queue ID {queue_id} was ignored."
                        ));
                    }
                }
                Err(error) => issues.push(format!("{name}: {error}")),
            }
        }
        self.entries = entries;
        self.issues = issues;
        Ok(())
    }

    pub fn enqueue(&mut self, entry: QueuedSubmission) -> Result<QueueInsertOutcome, String> {
        entry
            .validate()
            .map_err(|error| format!("submission queue entry is invalid: {error}"))?;
        let queue_id = entry.queue_id.to_string();
        if let Some(existing) = self.entries.get(&queue_id) {
            return if existing.file_byte_length == entry.file_byte_length
                && existing.canonical_content_sha256 == entry.canonical_content_sha256
                && existing.session == entry.session
            {
                Ok(QueueInsertOutcome::AlreadyQueued)
            } else {
                Err(format!(
                    "submission queue digest collision for artifact {queue_id}"
                ))
            };
        }
        if self.entries.len() >= MAXIMUM_QUEUE_ENTRIES {
            return Err(format!(
                "submission queue reached its {MAXIMUM_QUEUE_ENTRIES}-entry safety limit"
            ));
        }

        let final_path = self.directory.join(queue_file_name(&queue_id));
        if final_path.exists() {
            return Err(format!(
                "submission queue file already exists but was not loaded: {}",
                final_path.display()
            ));
        }
        let partial_path = self
            .directory
            .join(format!(".{queue_id}.submission.partial"));
        match std::fs::remove_file(&partial_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "could not replace interrupted submission partial {}: {error}",
                    partial_path.display()
                ));
            }
        }

        let mut encoded = serde_json::to_vec_pretty(&entry)
            .map_err(|error| format!("could not encode submission queue entry: {error}"))?;
        encoded.push(b'\n');
        if encoded.len() as u64 > MAXIMUM_QUEUE_ENTRY_BYTES {
            return Err(format!(
                "submission queue entry exceeds the {}-byte safety limit",
                MAXIMUM_QUEUE_ENTRY_BYTES
            ));
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial_path)
            .map_err(|error| {
                format!(
                    "could not create submission queue partial {}: {error}",
                    partial_path.display()
                )
            })?;
        let write_result = (|| {
            let mut writer = BufWriter::new(file);
            writer.write_all(&encoded)?;
            writer.flush()?;
            writer.get_ref().sync_all()
        })();
        if let Err(error) = write_result {
            let _ = std::fs::remove_file(&partial_path);
            return Err(format!("could not persist submission queue entry: {error}"));
        }
        if let Err(error) = std::fs::rename(&partial_path, &final_path) {
            let _ = std::fs::remove_file(&partial_path);
            return Err(format!(
                "could not publish submission queue entry {}: {error}",
                final_path.display()
            ));
        }
        if let Err(error) = sync_queue_directory(&self.directory) {
            self.reload()?;
            return Err(error);
        }

        self.entries.insert(queue_id, entry);
        Ok(QueueInsertOutcome::Queued)
    }

    pub fn entry(&self, queue_id: &str) -> Option<QueuedSubmission> {
        self.entries.get(queue_id).cloned()
    }

    pub fn snapshot(&self) -> SubmissionQueueView {
        let mut entries = self
            .entries
            .values()
            .map(QueuedSubmissionView::from)
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .created_unix_millis
                .cmp(&left.created_unix_millis)
                .then_with(|| left.queue_id.cmp(&right.queue_id))
        });
        SubmissionQueueView {
            schema_version: QUEUE_VIEW_SCHEMA_VERSION,
            queue_directory: self.directory.display().to_string(),
            entry_count: entries.len(),
            total_artifact_bytes: entries.iter().map(|entry| entry.file_byte_length).sum(),
            entries,
            issues: self.issues.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueInsertOutcome {
    Queued,
    AlreadyQueued,
}

impl QueueInsertOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::AlreadyQueued => "already_queued",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SubmissionQueueView {
    pub schema_version: u16,
    pub queue_directory: String,
    pub entry_count: usize,
    pub total_artifact_bytes: u64,
    pub entries: Vec<QueuedSubmissionView>,
    pub issues: Vec<String>,
}

impl Default for SubmissionQueueView {
    fn default() -> Self {
        Self {
            schema_version: QUEUE_VIEW_SCHEMA_VERSION,
            queue_directory: String::new(),
            entry_count: 0,
            total_artifact_bytes: 0,
            entries: Vec::new(),
            issues: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct QueuedSubmissionView {
    pub queue_id: String,
    pub created_unix_millis: u64,
    pub capture_session_id: String,
    pub local_artifact_path: String,
    pub artifact_exists: bool,
    pub artifact_byte_length_matches: bool,
    pub file_byte_length: u64,
    pub canonical_content_sha256: String,
    pub chunk_count: usize,
    pub state: SubmissionState,
    pub visibility: ReportVisibility,
    pub game_plugin_id: String,
    pub game_region: String,
    pub client_build: String,
}

impl From<&QueuedSubmission> for QueuedSubmissionView {
    fn from(entry: &QueuedSubmission) -> Self {
        let metadata = entry.session.metadata();
        let artifact_metadata = std::fs::metadata(&entry.local_artifact_path).ok();
        Self {
            queue_id: entry.queue_id.to_string(),
            created_unix_millis: entry.created_unix_millis,
            capture_session_id: entry.capture_session_id().to_owned(),
            local_artifact_path: entry.local_artifact_path.clone(),
            artifact_exists: artifact_metadata
                .as_ref()
                .is_some_and(std::fs::Metadata::is_file),
            artifact_byte_length_matches: artifact_metadata.as_ref().is_some_and(|metadata| {
                metadata.is_file() && metadata.len() == entry.file_byte_length
            }),
            file_byte_length: entry.file_byte_length,
            canonical_content_sha256: entry.canonical_content_sha256.to_string(),
            chunk_count: entry.session.chunks().len(),
            state: entry.state(),
            visibility: entry.visibility(),
            game_plugin_id: metadata.game_plugin_id.clone(),
            game_region: metadata.game_region.clone(),
            client_build: metadata.client_build.clone(),
        }
    }
}

fn load_entry(path: &Path, file_name: &str) -> Result<QueuedSubmission, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("could not inspect queue entry: {error}"))?;
    if !metadata.is_file() {
        return Err("queue entry is not a regular file".into());
    }
    if metadata.len() > MAXIMUM_QUEUE_ENTRY_BYTES {
        return Err(format!(
            "queue entry exceeds the {}-byte safety limit",
            MAXIMUM_QUEUE_ENTRY_BYTES
        ));
    }
    let mut file =
        File::open(path).map_err(|error| format!("could not open queue entry: {error}"))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("could not read queue entry: {error}"))?;
    let entry: QueuedSubmission = serde_json::from_slice(&bytes)
        .map_err(|error| format!("queue entry is invalid: {error}"))?;
    let expected_name = queue_file_name(entry.queue_id.as_str());
    if file_name != expected_name {
        return Err(format!(
            "queue filename does not match artifact digest; expected {expected_name}"
        ));
    }
    if entry.schema_version != QUEUED_SUBMISSION_SCHEMA_VERSION {
        return Err("queue entry schema was not validated".into());
    }
    Ok(entry)
}

fn queue_file_name(queue_id: &str) -> String {
    format!("{queue_id}{QUEUE_FILE_SUFFIX}")
}

#[cfg(unix)]
fn sync_queue_directory(directory: &Path) -> Result<(), String> {
    File::open(directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("could not sync submission queue directory: {error}"))
}

#[cfg(not(unix))]
fn sync_queue_directory(_: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use rlogs_events::{RegionContext, RegionIdentity};
    use rlogs_log_format::{RLOG_SCHEMA_VERSION, RlogHeader, RlogReplaySummary};
    use rlogs_submission::{
        LocalLogArtifact, LogChunkDescriptor, Sha256Digest, SubmissionMetadata,
    };

    use super::*;

    static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn temporary_directory() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("rlogs-submission-queue-{nanos}-{sequence}"))
    }

    fn digest(byte: &str) -> Sha256Digest {
        Sha256Digest::parse(byte.repeat(64)).unwrap()
    }

    fn queued_submission(path: &Path) -> QueuedSubmission {
        let artifact = LocalLogArtifact {
            header: RlogHeader {
                schema_version: RLOG_SCHEMA_VERSION,
                event_schema_version: 2,
                session_id: "capture-1".into(),
                region: RegionContext {
                    identity: RegionIdentity {
                        deployment_id: "global".into(),
                        region_id: "north-america".into(),
                        realm_id: None,
                        world_id: Some("asteria".into()),
                    },
                    client_build: "build-1".into(),
                    protocol_pack_digest: format!("sha256:{}", "a".repeat(64)),
                    evidence: Vec::new(),
                },
                producer: "test".into(),
            },
            rlog: RlogReplaySummary {
                event_count: 1,
                first_observed_micros: Some(1),
                last_observed_micros: Some(2),
                content_sha256: format!("sha256:{}", "b".repeat(64)),
            },
            file_byte_length: 12,
            file_sha256: digest("c"),
            chunks: vec![
                LogChunkDescriptor::new(0, 0, 8, digest("d")),
                LogChunkDescriptor::new(1, 8, 4, digest("e")),
            ],
        };
        let metadata = SubmissionMetadata::new(
            "app.rlogs.game.blue-protocol-star-resonance",
            "local-log-1",
            RLOG_SCHEMA_VERSION,
            "capture-1",
            "north-america",
            "build-1",
            digest("a"),
            digest("a"),
            ReportVisibility::Unlisted,
        );
        QueuedSubmission::new_post_run(
            metadata,
            &artifact,
            path.display().to_string(),
            1_700_000_000_000,
        )
        .unwrap()
    }

    #[test]
    fn atomic_queue_entry_reloads_and_duplicate_enqueue_is_idempotent() {
        let root = temporary_directory();
        let artifact_path = root.join("capture-1.rlog");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&artifact_path, b"fixture").unwrap();
        let entry = queued_submission(&artifact_path);
        let mut queue = LocalSubmissionQueue::open(root.join("queue")).unwrap();

        assert_eq!(
            queue.enqueue(entry.clone()).unwrap(),
            QueueInsertOutcome::Queued
        );
        assert_eq!(
            queue.enqueue(entry.clone()).unwrap(),
            QueueInsertOutcome::AlreadyQueued
        );
        let mut same_artifact_elsewhere = entry.clone();
        same_artifact_elsewhere.created_unix_millis += 1;
        same_artifact_elsewhere.local_artifact_path =
            root.join("moved-capture-1.rlog").display().to_string();
        assert_eq!(
            queue.enqueue(same_artifact_elsewhere).unwrap(),
            QueueInsertOutcome::AlreadyQueued
        );
        let restored = LocalSubmissionQueue::open(root.join("queue")).unwrap();
        let snapshot = restored.snapshot();
        assert_eq!(snapshot.entry_count, 1);
        assert_eq!(snapshot.entries[0].queue_id, entry.queue_id.to_string());
        assert!(snapshot.entries[0].artifact_exists);
        assert!(snapshot.issues.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_and_misnamed_queue_files_are_reported_without_becoming_entries() {
        let root = temporary_directory();
        let queue_root = root.join("queue");
        std::fs::create_dir_all(&queue_root).unwrap();
        std::fs::write(queue_root.join("broken.submission.json"), b"{nope").unwrap();
        let entry = queued_submission(&root.join("capture-1.rlog"));
        std::fs::write(
            queue_root.join("wrong.submission.json"),
            serde_json::to_vec(&entry).unwrap(),
        )
        .unwrap();

        let queue = LocalSubmissionQueue::open(queue_root).unwrap();
        assert_eq!(queue.snapshot().entry_count, 0);
        assert_eq!(queue.snapshot().issues.len(), 2);

        std::fs::remove_dir_all(root).unwrap();
    }
}
