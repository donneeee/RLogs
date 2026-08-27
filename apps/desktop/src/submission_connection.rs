use std::path::{Path, PathBuf};

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
        read_device_token()
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
        let endpoint_url = endpoint_url.trim().to_owned();
        let device_token = device_token.trim().to_owned();
        validate_endpoint_text(&endpoint_url)?;
        validate_token(&device_token)?;

        write_device_token(&device_token)?;
        let settings = SubmissionConnectionSettings {
            schema_version: SCHEMA_VERSION,
            endpoint_url: Some(endpoint_url),
        };
        if let Err(error) = write(&self.path, &settings) {
            let _ = delete_device_token();
            return Err(error);
        }
        self.settings = settings;
        self.view()
    }

    pub fn disconnect(&mut self) -> Result<SubmissionConnectionView, String> {
        delete_device_token()?;
        let settings = SubmissionConnectionSettings {
            schema_version: SCHEMA_VERSION,
            endpoint_url: None,
        };
        write(&self.path, &settings)?;
        self.settings = settings;
        self.view()
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
    let mut bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("encode submission connection settings: {error}"))?;
    bytes.push(b'\n');
    std::fs::write(path, bytes)
        .map_err(|error| format!("write submission connection settings: {error}"))
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
    use super::*;

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
}
