use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::Duration,
};

use reqwest::{
    Url,
    blocking::{Client, RequestBuilder},
    redirect::Policy,
};
use rlogs_profiles::LocalProfilePackage;
use rlogs_submission::{
    LogChunkDescriptor, QueuedSubmission, ServerReportReceipt, Sha256Digest, SubmissionState,
    VerificationTier,
};
use serde::{Deserialize, Serialize};

const ENDPOINT_ENVIRONMENT_VARIABLE: &str = "RLOGS_SUBMISSION_API_URL";
const TOKEN_ENVIRONMENT_VARIABLE: &str = "RLOGS_SUBMISSION_DEVICE_TOKEN";
const MAXIMUM_UPLOAD_CHUNK_BYTES: usize = 16 * 1024 * 1024;
const MAXIMUM_PHOTO_WALL_IMAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub struct SubmissionTransport {
    endpoint: Url,
    device_token: Option<String>,
    client: Client,
}

#[derive(Clone, Debug, Serialize)]
pub struct SubmissionTransportResult {
    pub schema_version: u16,
    pub queue_id: String,
    pub capture_session_id: String,
    pub report_id: String,
    pub share_url: String,
    pub final_state: SubmissionState,
    pub verification_tier: VerificationTier,
    pub chunk_count: usize,
    pub uploaded_chunk_count: usize,
    pub uploaded_bytes: u64,
    pub resumed: bool,
    pub duplicate: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProfilePublishResult {
    pub schema_version: u16,
    pub profile_id: String,
    pub character_id: String,
    pub package_id: String,
    pub claimed: bool,
    pub duplicate: bool,
    pub module_inventory_count: usize,
    pub equipped_module_count: usize,
    pub profile_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PhotoAssetPublishResult {
    pub schema_version: u16,
    pub profile_id: String,
    pub photo_id: u32,
    pub byte_length: usize,
    pub sha256: String,
    pub media_type: String,
    pub image_path: String,
}

#[derive(Clone, Debug, Deserialize)]
struct DeviceAuthenticationResponse {
    schema_version: u16,
    submitter_id: String,
    device_id: String,
    authentication: String,
}

impl SubmissionTransport {
    pub fn from_environment() -> Result<Option<Self>, String> {
        let endpoint = match std::env::var(ENDPOINT_ENVIRONMENT_VARIABLE) {
            Ok(value) if !value.trim().is_empty() => value,
            Ok(_) | Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "could not read {ENDPOINT_ENVIRONMENT_VARIABLE}: {error}"
                ));
            }
        };
        let device_token = match std::env::var(TOKEN_ENVIRONMENT_VARIABLE) {
            Ok(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
            Ok(_) | Err(std::env::VarError::NotPresent) => None,
            Err(error) => {
                return Err(format!(
                    "could not read {TOKEN_ENVIRONMENT_VARIABLE}: {error}"
                ));
            }
        };
        Self::new(&endpoint, device_token.as_deref()).map(Some)
    }

    pub fn new(endpoint: &str, device_token: Option<&str>) -> Result<Self, String> {
        let mut endpoint = Url::parse(endpoint.trim()).map_err(|error| {
            format!("{ENDPOINT_ENVIRONMENT_VARIABLE} is not a valid URL: {error}")
        })?;
        if endpoint.query().is_some() || endpoint.fragment().is_some() {
            return Err(format!(
                "{ENDPOINT_ENVIRONMENT_VARIABLE} cannot contain a query or fragment"
            ));
        }
        if !endpoint.username().is_empty() || endpoint.password().is_some() {
            return Err(format!(
                "{ENDPOINT_ENVIRONMENT_VARIABLE} cannot contain embedded credentials"
            ));
        }
        let secure = endpoint.scheme() == "https";
        let loopback = endpoint.scheme() == "http"
            && endpoint
                .host_str()
                .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1"));
        if !secure && !loopback {
            return Err(format!(
                "{ENDPOINT_ENVIRONMENT_VARIABLE} must use HTTPS (plain HTTP is allowed only for a loopback receiver)"
            ));
        }
        if !endpoint.path().ends_with('/') {
            endpoint.set_path(&format!("{}/", endpoint.path()));
        }
        let device_token = device_token
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30 * 60))
            .user_agent(concat!("rLogs/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| format!("could not initialize submission transport: {error}"))?;
        Ok(Self {
            endpoint,
            device_token,
            client,
        })
    }

    pub fn endpoint_url(&self) -> String {
        self.endpoint.as_str().trim_end_matches('/').to_owned()
    }

    pub fn validate_device_authentication(&self) -> Result<(), String> {
        self.device_authentication().map(|_| ())
    }

    pub fn bind_live_profile_packages(
        &self,
        packages: &mut [LocalProfilePackage],
    ) -> Result<(), String> {
        let device_token = self
            .device_token
            .as_deref()
            .ok_or_else(|| "an rLogs app token is required".to_owned())?;
        let identity = self.device_authentication()?;
        for package in packages {
            package
                .bind_live_capture(&identity.device_id, device_token)
                .map_err(|error| format!("could not bind live profile evidence: {error}"))?;
        }
        Ok(())
    }

    fn device_authentication(&self) -> Result<DeviceAuthenticationResponse, String> {
        if self.device_token.is_none() {
            return Err("an rLogs app token is required".into());
        }
        let response: DeviceAuthenticationResponse = self
            .authorized(self.client.get(self.url("v1/auth/device")?))
            .send()
            .map_err(|error| {
                format!("submission receiver could not validate the app token: {error}")
            })?
            .error_for_status()
            .map_err(|error| format!("submission receiver rejected the app token: {error}"))?
            .json()
            .map_err(|error| {
                format!("submission receiver returned an invalid app-token receipt: {error}")
            })?;
        if response.schema_version != 1
            || response.authentication != "device_token"
            || validate_identifier(&response.submitter_id, "submitter ID").is_err()
            || validate_identifier(&response.device_id, "device ID").is_err()
        {
            return Err("submission receiver returned an invalid app-token receipt".into());
        }
        Ok(response)
    }

    pub fn upload(
        &self,
        entry: &QueuedSubmission,
        artifact_path: &Path,
    ) -> Result<SubmissionTransportResult, String> {
        entry
            .validate()
            .map_err(|error| format!("submission draft is invalid: {error}"))?;
        let begin: BeginUploadResponse = self
            .authorized(
                self.client
                    .post(self.url("v1/uploads")?)
                    .json(&entry.session.manifest()),
            )
            .send()
            .map_err(|error| format!("submission receiver could not begin the upload: {error}"))?
            .error_for_status()
            .map_err(|error| format!("submission receiver rejected the manifest: {error}"))?
            .json()
            .map_err(|error| {
                format!("submission receiver returned an invalid manifest response: {error}")
            })?;
        if begin.schema_version != 1 {
            return Err(format!(
                "submission receiver uses unsupported response schema {}",
                begin.schema_version
            ));
        }
        if let (Some(report_id), Some(share_url)) = (begin.existing_report_id, begin.share_url) {
            return Ok(self.result(
                entry,
                report_id,
                share_url,
                VerificationTier::Replayed,
                0,
                0,
                true,
                true,
            ));
        }
        let upload_id = begin
            .upload_id
            .ok_or_else(|| "submission receiver omitted the upload ID".to_owned())?;
        validate_identifier(&upload_id, "upload ID")?;
        let missing = begin.missing_chunks.into_iter().collect::<BTreeSet<_>>();
        if missing.iter().any(|sequence| {
            !entry
                .session
                .chunks()
                .iter()
                .any(|chunk| chunk.sequence == *sequence)
        }) {
            return Err("submission receiver requested an unknown chunk".into());
        }
        let resumed = missing.len() < entry.session.chunks().len();
        let mut file = File::open(artifact_path)
            .map_err(|error| format!("could not reopen verified artifact: {error}"))?;
        let mut uploaded_chunk_count = 0_usize;
        let mut uploaded_bytes = 0_u64;
        for chunk in entry
            .session
            .chunks()
            .iter()
            .filter(|chunk| missing.contains(&chunk.sequence))
        {
            let bytes = read_chunk(&mut file, chunk)?;
            let response: ChunkUploadResponse = self
                .authorized(
                    self.client
                        .put(
                            self.url(&format!("v1/uploads/{upload_id}/chunks/{}", chunk.sequence))?,
                        )
                        .body(bytes),
                )
                .send()
                .map_err(|error| format!("chunk {} upload failed: {error}", chunk.sequence))?
                .error_for_status()
                .map_err(|error| format!("chunk {} was rejected: {error}", chunk.sequence))?
                .json()
                .map_err(|error| {
                    format!(
                        "chunk {} acknowledgement was invalid: {error}",
                        chunk.sequence
                    )
                })?;
            if response.schema_version != 1
                || response.sequence != chunk.sequence
                || response.sha256 != chunk.sha256
            {
                return Err(format!(
                    "chunk {} acknowledgement did not match the sealed manifest",
                    chunk.sequence
                ));
            }
            uploaded_chunk_count += 1;
            uploaded_bytes = uploaded_bytes.saturating_add(chunk.byte_length);
        }
        let finalized: FinalizeUploadResponse = self
            .authorized(
                self.client
                    .post(self.url(&format!("v1/uploads/{upload_id}/finalize"))?),
            )
            .send()
            .map_err(|error| format!("submission receiver could not finalize the upload: {error}"))?
            .error_for_status()
            .map_err(|error| format!("submission receiver rejected finalization: {error}"))?
            .json()
            .map_err(|error| format!("submission receiver returned an invalid receipt: {error}"))?;
        if finalized.schema_version != 1 || finalized.accepted_log_digest != entry.queue_id {
            return Err("submission receipt did not match the sealed artifact".into());
        }
        let receipt = ServerReportReceipt {
            report_id: finalized.report_id.clone(),
            accepted_log_digest: finalized.accepted_log_digest,
            verification_tier: finalized.verification_tier,
        };
        let mut session = entry.session.clone();
        session
            .start_upload()
            .map_err(|error| format!("submission state could not start: {error}"))?;
        for chunk in session.chunks().to_vec() {
            session
                .acknowledge_chunk(chunk.sequence, &chunk.sha256)
                .map_err(|error| format!("submission acknowledgement failed: {error}"))?;
        }
        session
            .begin_finalization()
            .map_err(|error| format!("submission state could not finalize: {error}"))?;
        session
            .complete(receipt)
            .map_err(|error| format!("submission receipt was rejected: {error}"))?;
        Ok(self.result(
            entry,
            finalized.report_id,
            finalized.share_url,
            finalized.verification_tier,
            uploaded_chunk_count,
            uploaded_bytes,
            resumed,
            finalized.duplicate,
        ))
    }

    pub fn publish_profile(
        &self,
        package: &LocalProfilePackage,
    ) -> Result<ProfilePublishResult, String> {
        package
            .validate()
            .map_err(|error| format!("profile package is invalid: {error}"))?;
        let expected_endpoint = "/v1/games/blue-protocol-star-resonance/profiles";
        if package.request.relative_endpoint != expected_endpoint {
            return Err("profile package targets an unsupported endpoint".into());
        }
        let device_token = self
            .device_token
            .as_deref()
            .ok_or_else(|| "an rLogs app token is required".to_owned())?;
        let identity = self.device_authentication()?;
        if !package.verifies_live_capture(&identity.device_id, device_token) {
            return Err(
                "profile package is not bound to this device's live process-owned capture".into(),
            );
        }
        let response: ProfilePublishResult = self
            .authorized(
                self.client
                    .post(self.url(expected_endpoint.trim_start_matches('/'))?)
                    .json(package),
            )
            .send()
            .map_err(|error| format!("profile receiver could not publish the package: {error}"))?
            .error_for_status()
            .map_err(|error| format!("profile receiver rejected the package: {error}"))?
            .json()
            .map_err(|error| format!("profile receiver returned an invalid receipt: {error}"))?;
        if response.schema_version != 1
            || response.package_id != package.package_id
            || response.character_id != package.request.payload.routing["character-id"]
            || !response.claimed
        {
            return Err("profile publication receipt did not match the sealed package".into());
        }
        Ok(response)
    }

    pub fn publish_profile_photo_from_source(
        &self,
        profile_id: &str,
        photo_id: u32,
        source_url: &str,
        declared_size: Option<u32>,
    ) -> Result<PhotoAssetPublishResult, String> {
        validate_profile_id(profile_id)?;
        if photo_id == 0 {
            return Err("Photo Wall photo ID must be positive".into());
        }
        if declared_size.is_some_and(|size| size as usize > MAXIMUM_PHOTO_WALL_IMAGE_BYTES) {
            return Err("Photo Wall image exceeds the publication size limit".into());
        }
        let source_url = reviewed_photo_source_url(source_url)?;
        // Never reuse the authenticated receiver request for the game CDN: the
        // app token must be sent only to the configured rLogs origin.
        let source_client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(Policy::none())
            .user_agent(concat!("rLogs/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| format!("could not create Photo Wall fetch client: {error}"))?;
        let source = source_client
            .get(source_url)
            .send()
            .map_err(|error| format!("could not fetch the reviewed Photo Wall image: {error}"))?
            .error_for_status()
            .map_err(|error| format!("the reviewed Photo Wall image was unavailable: {error}"))?;
        if source
            .content_length()
            .is_some_and(|length| length > MAXIMUM_PHOTO_WALL_IMAGE_BYTES as u64)
        {
            return Err("Photo Wall image exceeds the publication size limit".into());
        }
        let mut bytes = Vec::new();
        source
            .take(MAXIMUM_PHOTO_WALL_IMAGE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("could not read the reviewed Photo Wall image: {error}"))?;
        if bytes.is_empty() || bytes.len() > MAXIMUM_PHOTO_WALL_IMAGE_BYTES {
            return Err("Photo Wall image was empty or exceeded the publication size limit".into());
        }
        let endpoint = format!(
            "v1/games/blue-protocol-star-resonance/profiles/{profile_id}/photo-wall/{photo_id}"
        );
        let response: PhotoAssetPublishResult = self
            .authorized(
                self.client
                    .put(self.url(&endpoint)?)
                    .timeout(Duration::from_secs(60))
                    .body(bytes),
            )
            .send()
            .map_err(|error| format!("profile receiver could not publish the photo: {error}"))?
            .error_for_status()
            .map_err(|error| format!("profile receiver rejected the photo: {error}"))?
            .json()
            .map_err(|error| {
                format!("profile receiver returned an invalid photo receipt: {error}")
            })?;
        if response.schema_version != 1
            || response.profile_id != profile_id
            || response.photo_id != photo_id
            || response.byte_length == 0
            || response.sha256.len() != 64
            || response.image_path != format!("/v1/profiles/{profile_id}/photo-wall/{photo_id}")
        {
            return Err("Photo Wall publication receipt did not match the requested asset".into());
        }
        Ok(response)
    }

    #[allow(clippy::too_many_arguments)]
    fn result(
        &self,
        entry: &QueuedSubmission,
        report_id: String,
        share_url: String,
        verification_tier: VerificationTier,
        uploaded_chunk_count: usize,
        uploaded_bytes: u64,
        resumed: bool,
        duplicate: bool,
    ) -> SubmissionTransportResult {
        SubmissionTransportResult {
            schema_version: 1,
            queue_id: entry.queue_id.to_string(),
            capture_session_id: entry.capture_session_id().to_owned(),
            report_id,
            share_url,
            final_state: SubmissionState::Submitted,
            verification_tier,
            chunk_count: entry.session.chunks().len(),
            uploaded_chunk_count,
            uploaded_bytes,
            resumed,
            duplicate,
        }
    }

    fn url(&self, relative: &str) -> Result<Url, String> {
        self.endpoint
            .join(relative)
            .map_err(|error| format!("could not build submission URL: {error}"))
    }

    fn authorized(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.device_token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }
}

fn validate_profile_id(profile_id: &str) -> Result<(), String> {
    if profile_id.len() == 36
        && profile_id.starts_with("prf_")
        && profile_id[4..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err("published profile ID is invalid".into())
    }
}

fn reviewed_photo_source_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| "reviewed Photo Wall URL is invalid".to_owned())?;
    if url.scheme() != "https"
        || url.host_str() != Some("photo.playbpsr.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !url.path().starts_with("/xinghen-prod/")
        || url.fragment().is_some()
    {
        return Err("reviewed Photo Wall URL is outside the approved BPSR image origin".into());
    }
    Ok(url)
}

#[derive(Debug, Deserialize)]
struct BeginUploadResponse {
    schema_version: u16,
    upload_id: Option<String>,
    missing_chunks: Vec<u64>,
    existing_report_id: Option<String>,
    share_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChunkUploadResponse {
    schema_version: u16,
    sequence: u64,
    sha256: Sha256Digest,
}

#[derive(Debug, Deserialize)]
struct FinalizeUploadResponse {
    schema_version: u16,
    report_id: String,
    accepted_log_digest: Sha256Digest,
    verification_tier: VerificationTier,
    share_url: String,
    duplicate: bool,
}

fn read_chunk(file: &mut File, chunk: &LogChunkDescriptor) -> Result<Vec<u8>, String> {
    let byte_length = usize::try_from(chunk.byte_length)
        .map_err(|_| format!("chunk {} is too large for this platform", chunk.sequence))?;
    if byte_length == 0 || byte_length > MAXIMUM_UPLOAD_CHUNK_BYTES {
        return Err(format!(
            "chunk {} exceeds the {}-byte upload limit",
            chunk.sequence, MAXIMUM_UPLOAD_CHUNK_BYTES
        ));
    }
    file.seek(SeekFrom::Start(chunk.file_offset))
        .map_err(|error| format!("could not seek to chunk {}: {error}", chunk.sequence))?;
    let mut bytes = vec![0_u8; byte_length];
    file.read_exact(&mut bytes)
        .map_err(|error| format!("could not read chunk {}: {error}", chunk.sequence))?;
    Ok(bytes)
}

fn validate_identifier(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        Err(format!("submission receiver returned an invalid {label}"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_device_auth_response(
        status: u16,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let read = std::io::Read::read(&mut stream, &mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("GET /v1/auth/device HTTP/1.1"));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer rld_test")
            );
            let reason = if status == 200 { "OK" } else { "Unauthorized" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            std::io::Write::write_all(&mut stream, response.as_bytes()).unwrap();
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn identifiers_reject_paths_and_empty_values() {
        assert!(validate_identifier("upload_0123-ab", "upload ID").is_ok());
        assert!(validate_identifier("", "upload ID").is_err());
        assert!(validate_identifier("../artifact", "upload ID").is_err());
    }

    #[test]
    fn endpoints_require_https_except_for_loopback_development() {
        assert!(SubmissionTransport::new("https://receiver.example.com", Some("token")).is_ok());
        assert!(SubmissionTransport::new("http://127.0.0.1:8787", Some("token")).is_ok());
        assert!(SubmissionTransport::new("http://localhost:8787", Some("token")).is_ok());
        assert!(SubmissionTransport::new("http://receiver.example.com", Some("token")).is_err());
        assert!(
            SubmissionTransport::new("https://receiver.example.com?a=b", Some("token")).is_err()
        );
        assert!(
            SubmissionTransport::new("https://token@receiver.example.com", Some("token")).is_err()
        );
    }

    #[test]
    fn photo_sources_are_limited_to_the_reviewed_bpsr_image_origin() {
        assert!(
            reviewed_photo_source_url(
                "https://photo.playbpsr.com/xinghen-prod/1/3296036/photo.png"
            )
            .is_ok()
        );
        assert!(reviewed_photo_source_url("http://photo.playbpsr.com/xinghen-prod/a.png").is_err());
        assert!(reviewed_photo_source_url("https://example.com/xinghen-prod/a.png").is_err());
        assert!(reviewed_photo_source_url("https://photo.playbpsr.com/private/a.png").is_err());
    }

    #[test]
    fn device_authentication_is_verified_before_connection_storage() {
        let (endpoint, server) = mock_device_auth_response(
            200,
            r#"{"schema_version":1,"submitter_id":"sub_test","device_id":"device_test","authentication":"device_token"}"#,
        );
        SubmissionTransport::new(&endpoint, Some("rld_test"))
            .unwrap()
            .validate_device_authentication()
            .unwrap();
        server.join().unwrap();

        let (endpoint, server) = mock_device_auth_response(401, r#"{"error":"unauthorized"}"#);
        let error = SubmissionTransport::new(&endpoint, Some("rld_test"))
            .unwrap()
            .validate_device_authentication()
            .unwrap_err();
        assert!(error.contains("rejected the app token"));
        server.join().unwrap();
    }
}
