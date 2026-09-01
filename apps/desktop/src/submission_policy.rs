use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use rlogs_submission::ReportVisibility;
use serde::{Deserialize, Serialize};

pub const SUBMISSION_POLICY_SCHEMA_VERSION: u16 = 1;
const MAXIMUM_SUBMISSION_POLICY_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmissionPolicy {
    pub schema_version: u16,
    pub log_uploader: LogUploaderPolicy,
    pub bpsr_profile_sync: ProfileSyncPolicy,
}

impl SubmissionPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SUBMISSION_POLICY_SCHEMA_VERSION {
            return Err(format!(
                "submission policy schema {} is unsupported; expected {}",
                self.schema_version, SUBMISSION_POLICY_SCHEMA_VERSION
            ));
        }
        Ok(())
    }
}

impl Default for SubmissionPolicy {
    fn default() -> Self {
        Self {
            schema_version: SUBMISSION_POLICY_SCHEMA_VERSION,
            log_uploader: LogUploaderPolicy::default(),
            bpsr_profile_sync: ProfileSyncPolicy::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogUploaderPolicy {
    pub enabled: bool,
    pub automatic_combat_logs: bool,
    pub default_visibility: ReportVisibility,
    pub successful_artifact_retention: SuccessfulArtifactRetention,
}

impl Default for LogUploaderPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            automatic_combat_logs: true,
            default_visibility: ReportVisibility::Unlisted,
            successful_artifact_retention: SuccessfulArtifactRetention::Keep,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSyncPolicy {
    pub enabled: bool,
    pub automatic_profiles: bool,
    /// Photo Wall images are substantially more personal than ordinary game
    /// progression and therefore require a second, explicit opt-in.
    #[serde(default)]
    pub publish_photo_wall_images: bool,
}

impl Default for ProfileSyncPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            automatic_profiles: true,
            publish_photo_wall_images: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuccessfulArtifactRetention {
    Keep,
    RemoveAfterVerifiedReceipt,
}

#[derive(Clone, Debug, Serialize)]
pub struct SubmissionPolicyView {
    pub schema_version: u16,
    pub settings_path: String,
    pub transport_mode: &'static str,
    pub endpoint_url: Option<String>,
    pub log_uploader: LogUploaderPolicy,
    pub bpsr_profile_sync: ProfileSyncPolicy,
    pub issue: Option<String>,
}

#[derive(Debug)]
pub struct SubmissionPolicyStore {
    path: PathBuf,
    policy: SubmissionPolicy,
    issue: Option<String>,
}

impl SubmissionPolicyStore {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let parent = path
            .parent()
            .ok_or_else(|| "submission policy path has no parent".to_owned())?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create submission settings directory: {error}"))?;
        let (policy, issue) = load_policy(&path);
        Ok(Self {
            path,
            policy,
            issue,
        })
    }

    pub fn policy(&self) -> &SubmissionPolicy {
        &self.policy
    }

    pub fn snapshot(&self) -> SubmissionPolicyView {
        SubmissionPolicyView {
            schema_version: SUBMISSION_POLICY_SCHEMA_VERSION,
            settings_path: self.path.display().to_string(),
            transport_mode: "disconnected",
            endpoint_url: None,
            log_uploader: self.policy.log_uploader.clone(),
            bpsr_profile_sync: self.policy.bpsr_profile_sync.clone(),
            issue: self.issue.clone(),
        }
    }

    pub fn update(&mut self, policy: SubmissionPolicy) -> Result<SubmissionPolicyView, String> {
        policy.validate()?;
        save_policy(&self.path, &policy)?;
        self.policy = policy;
        self.issue = None;
        Ok(self.snapshot())
    }
}

fn load_policy(path: &Path) -> (SubmissionPolicy, Option<String>) {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (SubmissionPolicy::default(), None);
        }
        Err(error) => {
            return (
                SubmissionPolicy::default(),
                Some(format!("Could not inspect submission policy: {error}")),
            );
        }
    };
    if !metadata.is_file() || metadata.len() > MAXIMUM_SUBMISSION_POLICY_BYTES {
        return (
            SubmissionPolicy::default(),
            Some("Submission policy is not a bounded regular file.".into()),
        );
    }
    let result = (|| {
        let mut file = File::open(path)
            .map_err(|error| format!("could not open submission policy: {error}"))?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut bytes)
            .map_err(|error| format!("could not read submission policy: {error}"))?;
        let policy: SubmissionPolicy = serde_json::from_slice(&bytes)
            .map_err(|error| format!("submission policy is invalid: {error}"))?;
        policy.validate()?;
        Ok::<_, String>(policy)
    })();
    match result {
        Ok(policy) => (policy, None),
        Err(error) => (SubmissionPolicy::default(), Some(error)),
    }
}

fn save_policy(path: &Path, policy: &SubmissionPolicy) -> Result<(), String> {
    let partial_path = path.with_extension("json.partial");
    match std::fs::remove_file(&partial_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "could not replace interrupted submission settings partial: {error}"
            ));
        }
    }
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&partial_path)
        .map_err(|error| format!("could not create submission settings partial: {error}"))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, policy)
        .map_err(|error| format!("could not encode submission policy: {error}"))?;
    writer
        .write_all(b"\n")
        .and_then(|_| writer.flush())
        .and_then(|_| writer.get_ref().sync_all())
        .map_err(|error| format!("could not sync submission policy: {error}"))?;
    drop(writer);
    if let Err(error) = atomic_replace(&partial_path, path) {
        let _ = std::fs::remove_file(&partial_path);
        return Err(error);
    }
    sync_parent_directory(path)?;
    Ok(())
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(format!(
            "could not atomically publish submission policy: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::rename(source, destination)
        .map_err(|error| format!("could not atomically publish submission policy: {error}"))
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "submission policy path has no parent".to_owned())?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("could not sync submission settings directory: {error}"))
}

#[cfg(not(unix))]
fn sync_parent_directory(_: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn temporary_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!("rlogs-submission-policy-{nanos}-{sequence}"))
            .join("submission-policy.v1.json")
    }

    #[test]
    fn defaults_are_disabled_and_updates_round_trip_atomically() {
        let path = temporary_path();
        let mut store = SubmissionPolicyStore::open(path.clone()).unwrap();
        assert!(!store.policy().log_uploader.enabled);
        assert!(!store.policy().bpsr_profile_sync.enabled);
        assert!(!store.policy().bpsr_profile_sync.publish_photo_wall_images);
        assert_eq!(store.snapshot().transport_mode, "disconnected");

        let mut policy = store.policy().clone();
        policy.log_uploader.enabled = true;
        policy.bpsr_profile_sync.enabled = true;
        policy.bpsr_profile_sync.publish_photo_wall_images = true;
        store.update(policy.clone()).unwrap();
        let restored = SubmissionPolicyStore::open(path.clone()).unwrap();
        assert_eq!(restored.policy(), &policy);
        assert!(restored.snapshot().issue.is_none());

        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn corrupt_or_unknown_settings_fail_closed() {
        let path = temporary_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            br#"{"schema_version":1,"log_uploader":{"enabled":true},"unknown":true}"#,
        )
        .unwrap();
        let store = SubmissionPolicyStore::open(path.clone()).unwrap();

        assert!(!store.policy().log_uploader.enabled);
        assert!(!store.policy().bpsr_profile_sync.enabled);
        assert!(store.snapshot().issue.is_some());

        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
