use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;
const MAXIMUM_SETTINGS_BYTES: u64 = 64 * 1024;
const MAXIMUM_ENDPOINT_BYTES: usize = 2 * 1024;
const MAXIMUM_DEVICE_TOKEN_BYTES: usize = 1024;
const CREDENTIAL_TARGET: &str = "rLogs/submission-device-token/v1";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionConnectionSettings {
    schema_version: u16,
    endpoint_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionConnectionView {
    pub schema_version: u16,
    pub endpoint_url: Option<String>,
    pub credential_present: bool,
    pub credential_store: &'static str,
}

#[derive(Debug)]
pub struct SubmissionConnectionStore {
    path: PathBuf,
    settings: SubmissionConnectionSettings,
}

trait DeviceCredentialStore {
    fn read(&self) -> Result<Option<String>, String>;
    fn write(&self, token: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

struct OsDeviceCredentialStore;

impl DeviceCredentialStore for OsDeviceCredentialStore {
    fn read(&self) -> Result<Option<String>, String> {
        read_device_token()
    }

    fn write(&self, token: &str) -> Result<(), String> {
        write_device_token(token)
    }

    fn delete(&self) -> Result<(), String> {
        delete_device_token()
    }
}

impl SubmissionConnectionStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let settings = load(&path)?;
        Ok(Self { path, settings })
    }

    pub fn endpoint_url(&self) -> Option<&str> {
        self.settings.endpoint_url.as_deref()
    }

    pub fn device_token(&self) -> Result<Option<String>, String> {
        OsDeviceCredentialStore.read()
    }

    pub fn view(&self) -> Result<SubmissionConnectionView, String> {
        Ok(SubmissionConnectionView {
            schema_version: SCHEMA_VERSION,
            endpoint_url: self.settings.endpoint_url.clone(),
            credential_present: self.device_token()?.is_some(),
            credential_store: credential_store_name(),
        })
    }

    pub fn update(
        &mut self,
        endpoint_url: String,
        device_token: String,
    ) -> Result<SubmissionConnectionView, String> {
        self.update_with_credential_store(endpoint_url, device_token, &OsDeviceCredentialStore)
    }

    fn update_with_credential_store(
        &mut self,
        endpoint_url: String,
        device_token: String,
        credentials: &impl DeviceCredentialStore,
    ) -> Result<SubmissionConnectionView, String> {
        let endpoint_url = endpoint_url.trim().to_owned();
        let device_token = device_token.trim().to_owned();
        validate_endpoint_text(&endpoint_url)?;
        validate_token(&device_token)?;

        let previous_token = credentials.read()?;
        credentials.write(&device_token)?;
        let settings = SubmissionConnectionSettings {
            schema_version: SCHEMA_VERSION,
            endpoint_url: Some(endpoint_url),
        };
        if let Err(error) = write(&self.path, &settings) {
            return Err(rollback_error(
                error,
                restore_device_token(credentials, previous_token.as_deref()),
            ));
        }
        self.settings = settings;
        self.view_with_credential_store(credentials)
    }

    pub fn disconnect(&mut self) -> Result<SubmissionConnectionView, String> {
        self.disconnect_with_credential_store(&OsDeviceCredentialStore)
    }

    fn disconnect_with_credential_store(
        &mut self,
        credentials: &impl DeviceCredentialStore,
    ) -> Result<SubmissionConnectionView, String> {
        let previous_token = credentials.read()?;
        credentials.delete()?;
        let settings = SubmissionConnectionSettings {
            schema_version: SCHEMA_VERSION,
            endpoint_url: None,
        };
        if let Err(error) = write(&self.path, &settings) {
            return Err(rollback_error(
                error,
                restore_device_token(credentials, previous_token.as_deref()),
            ));
        }
        self.settings = settings;
        self.view_with_credential_store(credentials)
    }

    fn view_with_credential_store(
        &self,
        credentials: &impl DeviceCredentialStore,
    ) -> Result<SubmissionConnectionView, String> {
        Ok(SubmissionConnectionView {
            schema_version: SCHEMA_VERSION,
            endpoint_url: self.settings.endpoint_url.clone(),
            credential_present: credentials.read()?.is_some(),
            credential_store: credential_store_name(),
        })
    }
}

fn restore_device_token(
    credentials: &impl DeviceCredentialStore,
    previous_token: Option<&str>,
) -> Result<(), String> {
    match previous_token {
        Some(token) => credentials.write(token),
        None => credentials.delete(),
    }
}

fn rollback_error(primary: String, rollback: Result<(), String>) -> String {
    match rollback {
        Ok(()) => primary,
        Err(rollback) => format!("{primary}; credential rollback also failed: {rollback}"),
    }
}

fn load(path: &Path) -> Result<SubmissionConnectionSettings, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SubmissionConnectionSettings {
                schema_version: SCHEMA_VERSION,
                endpoint_url: None,
            });
        }
        Err(error) => return Err(format!("inspect submission connection settings: {error}")),
    };
    if metadata.len() > MAXIMUM_SETTINGS_BYTES {
        return Err("submission connection settings exceed 64 KiB".into());
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("read submission connection settings: {error}"))?;
    let settings: SubmissionConnectionSettings = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse submission connection settings: {error}"))?;
    if settings.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported submission connection schema {}; expected {SCHEMA_VERSION}",
            settings.schema_version
        ));
    }
    if let Some(endpoint) = settings.endpoint_url.as_deref() {
        validate_endpoint_text(endpoint)?;
    }
    Ok(settings)
}

fn write(path: &Path, settings: &SubmissionConnectionSettings) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "submission connection settings path has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create submission connection settings folder: {error}"))?;
    let partial = path.with_extension("json.partial");
    match std::fs::remove_file(&partial) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "remove interrupted submission connection partial: {error}"
            ));
        }
    }
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&partial)
        .map_err(|error| format!("create submission connection partial: {error}"))?;
    let mut writer = BufWriter::new(file);
    if let Err(error) = serde_json::to_writer_pretty(&mut writer, settings)
        .map_err(|error| format!("encode submission connection settings: {error}"))
        .and_then(|_| {
            writer
                .write_all(b"\n")
                .and_then(|_| writer.flush())
                .and_then(|_| writer.get_ref().sync_all())
                .map_err(|error| format!("sync submission connection settings: {error}"))
        })
    {
        drop(writer);
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    drop(writer);
    if let Err(error) = atomic_replace(&partial, path) {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    sync_parent_directory(path)
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
    // SAFETY: both vectors are valid nul-terminated UTF-16 paths for the
    // duration of the call.
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(format!(
            "atomically publish submission connection settings: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::rename(source, destination)
        .map_err(|error| format!("atomically publish submission connection settings: {error}"))
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "submission connection settings path has no parent".to_owned())?;
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("sync submission connection settings directory: {error}"))
}

#[cfg(not(unix))]
fn sync_parent_directory(_: &Path) -> Result<(), String> {
    Ok(())
}

fn validate_endpoint_text(endpoint: &str) -> Result<(), String> {
    if endpoint.is_empty() {
        return Err("submission receiver URL is required".into());
    }
    if endpoint.len() > MAXIMUM_ENDPOINT_BYTES || endpoint.contains('\0') {
        return Err("submission receiver URL is invalid or too long".into());
    }
    Ok(())
}

fn validate_token(token: &str) -> Result<(), String> {
    if token.is_empty() {
        return Err("device token is required".into());
    }
    if token.len() > MAXIMUM_DEVICE_TOKEN_BYTES || token.contains('\0') {
        return Err("device token is invalid or too long".into());
    }
    Ok(())
}

#[cfg(windows)]
fn credential_store_name() -> &'static str {
    "Windows Credential Manager"
}

#[cfg(not(windows))]
fn credential_store_name() -> &'static str {
    "unavailable in this platform build"
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn read_device_token() -> Result<Option<String>, String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Security::Credentials::{
        CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW,
    };

    let target = wide(CREDENTIAL_TARGET);
    let mut raw: *mut CREDENTIALW = null_mut();
    // SAFETY: `target` is nul-terminated and `raw` is an out pointer released
    // with CredFree on every successful CredReadW call.
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) } == 0 {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(1168) {
            Ok(None)
        } else {
            Err(format!(
                "read device token from Windows Credential Manager: {error}"
            ))
        };
    }
    // SAFETY: CredReadW returned a valid CREDENTIALW allocation. The blob is
    // valid for CredentialBlobSize bytes until CredFree is called below.
    let bytes = unsafe {
        let credential = &*raw;
        std::slice::from_raw_parts(
            credential.CredentialBlob,
            credential.CredentialBlobSize as usize,
        )
        .to_vec()
    };
    // SAFETY: `raw` came from a successful CredReadW call.
    unsafe { CredFree(raw.cast()) };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "Windows Credential Manager contains a non-UTF-8 device token".into())
}

#[cfg(windows)]
fn write_device_token(token: &str) -> Result<(), String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredWriteW,
    };

    let mut target = wide(CREDENTIAL_TARGET);
    let mut username = wide("rLogs submission device");
    let mut bytes = token.as_bytes().to_vec();
    let blob_size = u32::try_from(bytes.len()).map_err(|_| "device token is too long")?;
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        CredentialBlobSize: blob_size,
        CredentialBlob: bytes.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: username.as_mut_ptr(),
        Comment: null_mut(),
        TargetAlias: null_mut(),
        ..CREDENTIALW::default()
    };
    // SAFETY: all pointers in `credential` remain valid for the duration of
    // CredWriteW and their lengths are recorded correctly.
    let written = unsafe { CredWriteW(&credential, 0) };
    bytes.fill(0);
    if written == 0 {
        return Err(format!(
            "write device token to Windows Credential Manager: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn delete_device_token() -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::{CRED_TYPE_GENERIC, CredDeleteW};

    let target = wide(CREDENTIAL_TARGET);
    // SAFETY: `target` is a valid nul-terminated UTF-16 string.
    if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(1168) {
            return Err(format!(
                "remove device token from Windows Credential Manager: {error}"
            ));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn read_device_token() -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(not(windows))]
fn write_device_token(_token: &str) -> Result<(), String> {
    Err("this platform build does not yet provide an OS credential-vault adapter".into())
}

#[cfg(not(windows))]
fn delete_device_token() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct TemporaryDirectory(PathBuf);

    impl TemporaryDirectory {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("rlogs-submission-connection-{nanos}-{sequence}"));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Default)]
    struct MemoryCredentialStore {
        token: RefCell<Option<String>>,
    }

    impl MemoryCredentialStore {
        fn with_token(token: &str) -> Self {
            Self {
                token: RefCell::new(Some(token.to_owned())),
            }
        }
    }

    impl DeviceCredentialStore for MemoryCredentialStore {
        fn read(&self) -> Result<Option<String>, String> {
            Ok(self.token.borrow().clone())
        }

        fn write(&self, token: &str) -> Result<(), String> {
            *self.token.borrow_mut() = Some(token.to_owned());
            Ok(())
        }

        fn delete(&self) -> Result<(), String> {
            *self.token.borrow_mut() = None;
            Ok(())
        }
    }

    #[test]
    fn endpoint_settings_never_serialize_a_token() {
        let settings = SubmissionConnectionSettings {
            schema_version: SCHEMA_VERSION,
            endpoint_url: Some("https://receiver.example.com".into()),
        };
        let encoded = serde_json::to_string(&settings).unwrap();
        assert!(encoded.contains("receiver.example.com"));
        assert!(!encoded.contains("token"));
        assert!(!encoded.contains("credential"));
    }

    #[test]
    fn empty_or_oversized_values_are_rejected() {
        assert!(validate_endpoint_text("").is_err());
        assert!(validate_token("").is_err());
        assert!(validate_token(&"x".repeat(MAXIMUM_DEVICE_TOKEN_BYTES + 1)).is_err());
    }

    #[test]
    fn connection_update_atomically_persists_endpoint_without_serializing_token() {
        let root = TemporaryDirectory::new();
        let path = root.path().join("settings/connection.json");
        let credentials = MemoryCredentialStore::default();
        let mut store = SubmissionConnectionStore::open(&path).unwrap();

        let view = store
            .update_with_credential_store(
                "https://receiver.example.com".into(),
                "secret-device-token".into(),
                &credentials,
            )
            .unwrap();

        assert!(view.credential_present);
        assert_eq!(
            credentials.read().unwrap().as_deref(),
            Some("secret-device-token")
        );
        let bytes = std::fs::read_to_string(&path).unwrap();
        assert!(bytes.contains("receiver.example.com"));
        assert!(!bytes.contains("secret-device-token"));
        assert!(!path.with_extension("json.partial").exists());
        assert_eq!(
            SubmissionConnectionStore::open(path)
                .unwrap()
                .endpoint_url(),
            Some("https://receiver.example.com")
        );
    }

    #[test]
    fn failed_endpoint_write_restores_the_previous_credential() {
        let root = TemporaryDirectory::new();
        let blocked_parent = root.path().join("not-a-directory");
        std::fs::write(&blocked_parent, b"block directory creation").unwrap();
        let credentials = MemoryCredentialStore::with_token("previous-token");
        let mut store = SubmissionConnectionStore {
            path: blocked_parent.join("connection.json"),
            settings: SubmissionConnectionSettings {
                schema_version: SCHEMA_VERSION,
                endpoint_url: Some("https://old.example.com".into()),
            },
        };

        let error = store
            .update_with_credential_store(
                "https://new.example.com".into(),
                "replacement-token".into(),
                &credentials,
            )
            .unwrap_err();

        assert!(error.contains("submission connection settings folder"));
        assert_eq!(
            credentials.read().unwrap().as_deref(),
            Some("previous-token")
        );
        assert_eq!(store.endpoint_url(), Some("https://old.example.com"));
    }

    #[test]
    fn failed_disconnect_write_restores_the_connected_credential() {
        let root = TemporaryDirectory::new();
        let blocked_parent = root.path().join("not-a-directory");
        std::fs::write(&blocked_parent, b"block directory creation").unwrap();
        let credentials = MemoryCredentialStore::with_token("connected-token");
        let mut store = SubmissionConnectionStore {
            path: blocked_parent.join("connection.json"),
            settings: SubmissionConnectionSettings {
                schema_version: SCHEMA_VERSION,
                endpoint_url: Some("https://receiver.example.com".into()),
            },
        };

        let error = store
            .disconnect_with_credential_store(&credentials)
            .unwrap_err();

        assert!(error.contains("submission connection settings folder"));
        assert_eq!(
            credentials.read().unwrap().as_deref(),
            Some("connected-token")
        );
        assert_eq!(store.endpoint_url(), Some("https://receiver.example.com"));
    }
}
