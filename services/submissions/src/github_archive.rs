//! Optional private GitHub evidence archive.
//!
//! GitHub is a secondary research sink, never the source of truth for an
//! accepted submission. The validated receiver first persists the sealed
//! artifact and public projection locally, then records a retryable outbox
//! job. A background worker uploads immutable, digest-named release assets.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::Duration,
};

use reqwest::{
    StatusCode, Url,
    blocking::{Body, Client, RequestBuilder},
    header::{ACCEPT, CONTENT_TYPE, USER_AGENT},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const API_VERSION: &str = "2022-11-28";
const DEFAULT_PART_BYTES: u64 = 512 * 1024 * 1024;
const MINIMUM_PART_BYTES: u64 = 8 * 1024 * 1024;
const MAXIMUM_PART_BYTES: u64 = 1024 * 1024 * 1024;
const ARCHIVE_SCHEMA_VERSION: u16 = 1;

#[derive(Clone)]
pub(crate) struct GithubArchive {
    repository: String,
    api_root: Url,
    token: String,
    part_bytes: u64,
    client: Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArchiveJob {
    pub schema_version: u16,
    pub report_id: String,
    pub artifact_sha256: String,
    pub created_unix_millis: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArchiveReceipt {
    pub schema_version: u16,
    pub report_id: String,
    pub artifact_sha256: String,
    pub repository: String,
    pub release_id: u64,
    pub release_tag: String,
    pub release_url: String,
    pub archived_unix_millis: u64,
}

#[derive(Debug, Clone, Serialize)]
struct EvidenceManifest {
    schema_version: u16,
    report_id: String,
    artifact_sha256: String,
    artifact_byte_length: u64,
    projection: EvidenceAsset,
    artifact_parts: Vec<EvidencePart>,
}

#[derive(Debug, Clone, Serialize)]
struct EvidenceAsset {
    name: String,
    byte_length: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize)]
struct EvidencePart {
    sequence: u32,
    offset: u64,
    byte_length: u64,
    sha256: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    id: u64,
    html_url: String,
    upload_url: String,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    size: u64,
}

struct FilePart {
    file: File,
    remaining: u64,
}

impl Read for FilePart {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let maximum =
            usize::try_from(self.remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let read = self.file.read(&mut buffer[..maximum])?;
        self.remaining = self.remaining.saturating_sub(read as u64);
        Ok(read)
    }
}

impl GithubArchive {
    pub(crate) fn from_environment() -> Result<Option<Self>, String> {
        let repository = match std::env::var("RLOGS_GITHUB_ARCHIVE_REPOSITORY") {
            Ok(value) if !value.trim().is_empty() => value,
            Ok(_) | Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(error) => return Err(format!("read GitHub archive repository: {error}")),
        };
        let token = std::env::var("RLOGS_GITHUB_ARCHIVE_TOKEN").map_err(|_| {
            "RLOGS_GITHUB_ARCHIVE_TOKEN is required when the GitHub research archive is enabled"
                .to_string()
        })?;
        let api_root = std::env::var("RLOGS_GITHUB_API_URL")
            .unwrap_or_else(|_| "https://api.github.com/".into());
        let part_bytes = std::env::var("RLOGS_GITHUB_ARCHIVE_PART_BYTES")
            .ok()
            .map(|value| {
                value
                    .parse::<u64>()
                    .map_err(|error| format!("invalid GitHub archive part size: {error}"))
            })
            .transpose()?
            .unwrap_or(DEFAULT_PART_BYTES);
        Self::new(repository, token, &api_root, part_bytes).map(Some)
    }

    fn new(
        repository: String,
        token: String,
        api_root: &str,
        part_bytes: u64,
    ) -> Result<Self, String> {
        validate_repository(&repository)?;
        if token.trim().is_empty() {
            return Err("GitHub archive token cannot be empty".into());
        }
        if !(MINIMUM_PART_BYTES..=MAXIMUM_PART_BYTES).contains(&part_bytes) {
            return Err(format!(
                "GitHub archive part size must be between {MINIMUM_PART_BYTES} and {MAXIMUM_PART_BYTES} bytes"
            ));
        }
        let api_root =
            Url::parse(api_root).map_err(|error| format!("invalid GitHub API URL: {error}"))?;
        if api_root.scheme() != "https"
            && !(api_root.scheme() == "http"
                && api_root
                    .host_str()
                    .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1")))
        {
            return Err("GitHub API must use HTTPS except for loopback tests".into());
        }
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30 * 60))
            .build()
            .map_err(|error| format!("initialize GitHub archive client: {error}"))?;
        Ok(Self {
            repository,
            api_root,
            token,
            part_bytes,
            client,
        })
    }

    pub(crate) fn repository(&self) -> &str {
        &self.repository
    }

    pub(crate) fn archive(
        &self,
        job: &ArchiveJob,
        artifact_path: &Path,
        projection_path: &Path,
        archived_unix_millis: u64,
    ) -> Result<ArchiveReceipt, String> {
        let projection = std::fs::read(projection_path)
            .map_err(|error| format!("read public projection: {error}"))?;
        let projection_sha256 = hex_digest(&projection);
        let projection_name = format!("projection.v1.{projection_sha256}.json");
        let artifact_length = artifact_path
            .metadata()
            .map_err(|error| format!("inspect sealed artifact: {error}"))?
            .len();
        let parts = describe_parts(artifact_path, artifact_length, self.part_bytes)?;
        let manifest = EvidenceManifest {
            schema_version: ARCHIVE_SCHEMA_VERSION,
            report_id: job.report_id.clone(),
            artifact_sha256: job.artifact_sha256.clone(),
            artifact_byte_length: artifact_length,
            projection: EvidenceAsset {
                name: projection_name.clone(),
                byte_length: projection.len() as u64,
                sha256: projection_sha256,
            },
            artifact_parts: parts.clone(),
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| format!("encode evidence manifest: {error}"))?;
        let manifest_name = format!("evidence-manifest.v1.{}.json", hex_digest(&manifest_bytes));

        let tag = format!("evidence-sha256-{}", job.artifact_sha256);
        let mut release = self.get_or_create_release(&tag, job)?;
        let mut existing = release
            .assets
            .drain(..)
            .map(|asset| (asset.name, asset.size))
            .collect::<BTreeMap<_, _>>();

        for part in &parts {
            self.upload_file_part_if_missing(
                &release.upload_url,
                &mut existing,
                artifact_path,
                part,
            )?;
        }
        self.upload_bytes_if_missing(
            &release.upload_url,
            &mut existing,
            &projection_name,
            "application/json",
            projection,
        )?;
        self.upload_bytes_if_missing(
            &release.upload_url,
            &mut existing,
            &manifest_name,
            "application/json",
            manifest_bytes,
        )?;

        Ok(ArchiveReceipt {
            schema_version: ARCHIVE_SCHEMA_VERSION,
            report_id: job.report_id.clone(),
            artifact_sha256: job.artifact_sha256.clone(),
            repository: self.repository.clone(),
            release_id: release.id,
            release_tag: tag,
            release_url: release.html_url,
            archived_unix_millis,
        })
    }

    fn get_or_create_release(&self, tag: &str, job: &ArchiveJob) -> Result<GithubRelease, String> {
        let get_url = self
            .api_root
            .join(&format!("repos/{}/releases/tags/{tag}", self.repository))
            .map_err(|error| format!("build GitHub release lookup URL: {error}"))?;
        let response = self
            .authorized(self.client.get(get_url))
            .send()
            .map_err(|error| format!("look up GitHub evidence release: {error}"))?;
        if response.status().is_success() {
            return response
                .json()
                .map_err(|error| format!("decode GitHub evidence release: {error}"));
        }
        if response.status() != StatusCode::NOT_FOUND {
            return Err(github_response_error("look up evidence release", response));
        }

        let create_url = self
            .api_root
            .join(&format!("repos/{}/releases", self.repository))
            .map_err(|error| format!("build GitHub release creation URL: {error}"))?;
        let response = self
            .authorized(self.client.post(create_url))
            .json(&serde_json::json!({
                "tag_name": tag,
                "name": format!("rLogs evidence {}", &job.artifact_sha256[..12]),
                "body": format!(
                    "Validated rLogs research evidence. Report `{}`; full artifact SHA-256 `{}`. Assets are digest-named and reconstructable from the evidence manifest.",
                    job.report_id, job.artifact_sha256
                ),
                "draft": false,
                "prerelease": true,
                "generate_release_notes": false,
            }))
            .send()
            .map_err(|error| format!("create GitHub evidence release: {error}"))?;
        if response.status().is_success() {
            return response
                .json()
                .map_err(|error| format!("decode created GitHub evidence release: {error}"));
        }
        if response.status() == StatusCode::UNPROCESSABLE_ENTITY {
            let retry = self
                .authorized(
                    self.client.get(
                        self.api_root
                            .join(&format!("repos/{}/releases/tags/{tag}", self.repository))
                            .map_err(|error| format!("build release retry URL: {error}"))?,
                    ),
                )
                .send()
                .map_err(|error| format!("retry GitHub evidence release lookup: {error}"))?;
            if retry.status().is_success() {
                return retry
                    .json()
                    .map_err(|error| format!("decode existing GitHub evidence release: {error}"));
            }
        }
        Err(github_response_error("create evidence release", response))
    }

    fn upload_file_part_if_missing(
        &self,
        upload_template: &str,
        existing: &mut BTreeMap<String, u64>,
        artifact_path: &Path,
        part: &EvidencePart,
    ) -> Result<(), String> {
        if asset_exists(existing, &part.name, part.byte_length)? {
            return Ok(());
        }
        let mut file = File::open(artifact_path)
            .map_err(|error| format!("open sealed artifact part: {error}"))?;
        file.seek(SeekFrom::Start(part.offset))
            .map_err(|error| format!("seek sealed artifact part: {error}"))?;
        let body = Body::new(FilePart {
            file,
            remaining: part.byte_length,
        });
        let response = self
            .authorized(
                self.client
                    .post(upload_asset_url(upload_template, &part.name)?)
                    .header(CONTENT_TYPE, "application/octet-stream")
                    .header(reqwest::header::CONTENT_LENGTH, part.byte_length)
                    .body(body),
            )
            .send()
            .map_err(|error| format!("upload GitHub artifact part {}: {error}", part.sequence))?;
        if !response.status().is_success() {
            return Err(github_response_error("upload artifact part", response));
        }
        existing.insert(part.name.clone(), part.byte_length);
        Ok(())
    }

    fn upload_bytes_if_missing(
        &self,
        upload_template: &str,
        existing: &mut BTreeMap<String, u64>,
        name: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<(), String> {
        if asset_exists(existing, name, bytes.len() as u64)? {
            return Ok(());
        }
        let response = self
            .authorized(
                self.client
                    .post(upload_asset_url(upload_template, name)?)
                    .header(CONTENT_TYPE, content_type)
                    .body(bytes.clone()),
            )
            .send()
            .map_err(|error| format!("upload GitHub evidence asset {name}: {error}"))?;
        if !response.status().is_success() {
            return Err(github_response_error("upload evidence metadata", response));
        }
        existing.insert(name.to_owned(), bytes.len() as u64);
        Ok(())
    }

    fn authorized(&self, request: RequestBuilder) -> RequestBuilder {
        request
            .bearer_auth(&self.token)
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .header(
                USER_AGENT,
                concat!("rLogs-submission-archive/", env!("CARGO_PKG_VERSION")),
            )
    }
}

fn validate_repository(repository: &str) -> Result<(), String> {
    let mut parts = repository.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if owner.is_empty()
        || name.is_empty()
        || parts.next().is_some()
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("GitHub archive repository must use owner/name form".into());
    }
    Ok(())
}

fn describe_parts(path: &Path, total: u64, part_bytes: u64) -> Result<Vec<EvidencePart>, String> {
    let mut file = File::open(path).map_err(|error| format!("open sealed artifact: {error}"))?;
    let mut parts = Vec::new();
    let mut offset = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    while offset < total {
        let length = part_bytes.min(total - offset);
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| format!("seek sealed artifact: {error}"))?;
        let mut hasher = Sha256::new();
        let mut remaining = length;
        while remaining > 0 {
            let maximum =
                usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
            let read = file
                .read(&mut buffer[..maximum])
                .map_err(|error| format!("hash sealed artifact part: {error}"))?;
            if read == 0 {
                return Err("sealed artifact ended before its recorded byte length".into());
            }
            hasher.update(&buffer[..read]);
            remaining -= read as u64;
        }
        let sha256 = format!("{:x}", hasher.finalize());
        let sequence = u32::try_from(parts.len())
            .map_err(|_| "sealed artifact has too many GitHub parts".to_string())?;
        let name = format!("artifact.part{sequence:04}.{sha256}.bin");
        parts.push(EvidencePart {
            sequence,
            offset,
            byte_length: length,
            sha256,
            name,
        });
        offset += length;
    }
    Ok(parts)
}

fn asset_exists(
    existing: &BTreeMap<String, u64>,
    name: &str,
    expected: u64,
) -> Result<bool, String> {
    match existing.get(name) {
        Some(actual) if *actual == expected => Ok(true),
        Some(actual) => Err(format!(
            "GitHub evidence asset {name} has {actual} bytes; expected {expected}"
        )),
        None => Ok(false),
    }
}

fn upload_asset_url(template: &str, name: &str) -> Result<Url, String> {
    let base = template.split('{').next().unwrap_or(template);
    let mut url =
        Url::parse(base).map_err(|error| format!("invalid GitHub upload URL: {error}"))?;
    url.query_pairs_mut().append_pair("name", name);
    Ok(url)
}

fn github_response_error(context: &str, response: reqwest::blocking::Response) -> String {
    let status = response.status();
    let body = response.text().unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| value.get("message")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| "GitHub did not provide an error message".into());
    format!("{context}: GitHub returned {status}: {message}")
}

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_names_are_strict() {
        assert!(validate_repository("donneeee/rlogs-rdps-evidence").is_ok());
        assert!(validate_repository("missing-owner").is_err());
        assert!(validate_repository("owner/repo/extra").is_err());
        assert!(validate_repository("owner/repo?token=secret").is_err());
    }

    #[test]
    fn release_upload_template_becomes_a_named_asset_url() {
        let url = upload_asset_url(
            "https://uploads.github.com/repos/a/b/releases/1/assets{?name,label}",
            "artifact.part0000.abc.bin",
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://uploads.github.com/repos/a/b/releases/1/assets?name=artifact.part0000.abc.bin"
        );
    }

    #[test]
    fn artifact_parts_are_bounded_and_digest_named() {
        let directory = tempfile::tempdir().unwrap();
        let path: std::path::PathBuf = directory.path().join("sample.rlog");
        std::fs::write(&path, b"abcdefghijklmnop").unwrap();
        let parts = describe_parts(&path, 16, 8).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].offset, 0);
        assert_eq!(parts[0].byte_length, 8);
        assert!(parts[0].name.contains(&parts[0].sha256));
        assert_eq!(parts[1].offset, 8);
    }
}
