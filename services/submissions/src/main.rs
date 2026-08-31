use std::{
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Write, sink},
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

use rlogs_game_bpsr::BPSR_GAME_PLUGIN_ID;
use rlogs_log_format::{RlogLimits, RlogReader};
use rlogs_submission::{
    ArtifactBuildLimits, ReportVisibility, Sha256Digest, SubmissionMetadata, SubmissionSession,
    build_privacy_verified_submission_artifact, submission_privacy_policy_digest,
    write_privacy_filtered_submission_log,
};
use rlogs_submission_service::{SubmissionAuthentication, SubmissionService, router};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if let Some(index) = arguments
        .iter()
        .position(|argument| argument == "--prepare-submission")
    {
        let source_path = arguments
            .get(index + 1)
            .map(PathBuf::from)
            .ok_or("--prepare-submission requires a sealed source .rlog path")?;
        let artifact_path = arguments
            .get(index + 2)
            .map(PathBuf::from)
            .ok_or("--prepare-submission requires an artifact output path")?;
        let manifest_path = arguments
            .get(index + 3)
            .map(PathBuf::from)
            .ok_or("--prepare-submission requires a manifest output path")?;
        if arguments.len() != 4 || index != 0 {
            return Err(
                "usage: rlogs-submission-service --prepare-submission <sealed-source.rlog> <artifact-output.rlog> <manifest-output.json>"
                    .into(),
            );
        }
        prepare_submission(&source_path, &artifact_path, &manifest_path)?;
        return Ok(());
    }
    if let Some(index) = arguments
        .iter()
        .position(|argument| argument == "--audit-artifact")
    {
        let path = arguments
            .get(index + 1)
            .map(PathBuf::from)
            .ok_or("--audit-artifact requires a sealed .rlog path")?;
        if arguments.len() != 2 || index != 0 {
            return Err("usage: rlogs-submission-service --audit-artifact <sealed.rlog>".into());
        }
        let reader = RlogReader::new(BufReader::new(File::open(&path)?), RlogLimits::default())?;
        let network_identifier_evidence = reader
            .header()
            .region
            .evidence
            .iter()
            .filter(|evidence| contains_network_identifier(&evidence.reference))
            .count();
        let (_, _, summary) = write_privacy_filtered_submission_log(
            BufReader::new(File::open(&path)?),
            sink(),
            RlogLimits::default(),
        )?;
        if summary.excluded_chat_events != 0
            || summary.stripped_region_evidence_entries != 0
            || network_identifier_evidence != 0
        {
            return Err(format!(
                "artifact is not submission-privacy compliant: {} chat event(s), {} free-form region evidence value(s), {} value(s) containing an IP or MAC address; a new privacy export is required",
                summary.excluded_chat_events,
                summary.stripped_region_evidence_entries,
                network_identifier_evidence,
            )
            .into());
        }
        let artifact = build_privacy_verified_submission_artifact(
            File::open(&path)?,
            ArtifactBuildLimits::default(),
            RlogLimits::default(),
        )?;
        println!(
            "privacy audit passed: {} bytes, {} canonical events, sha256:{}",
            artifact.file_byte_length, artifact.rlog.event_count, artifact.file_sha256
        );
        return Ok(());
    }
    let archive_once = arguments
        .iter()
        .any(|argument| argument == "--archive-once");
    let data_root = std::env::var_os("RLOGS_SUBMISSION_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("runtime-data/submission-service"));
    let public_site_url =
        std::env::var("RLOGS_PUBLIC_SITE_URL").unwrap_or_else(|_| "http://127.0.0.1:5173".into());
    let ingest_key = std::env::var("RLOGS_INGEST_KEY").ok();
    let introspection_url = std::env::var("RLOGS_AUTH_INTROSPECTION_URL").ok();
    if ingest_key.is_some() && introspection_url.is_some() {
        return Err(
            "configure either RLOGS_AUTH_INTROSPECTION_URL or RLOGS_INGEST_KEY, not both".into(),
        );
    }
    let authentication = match (introspection_url, ingest_key) {
        (Some(endpoint), None) => SubmissionAuthentication::introspection(&endpoint)?,
        (None, Some(key)) => SubmissionAuthentication::shared_ingest_key(key)?,
        (None, None) => SubmissionAuthentication::UnauthenticatedDevelopment,
        (Some(_), Some(_)) => unreachable!("conflicting authentication was rejected above"),
    };
    let listen: SocketAddr = std::env::var("RLOGS_SUBMISSION_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:8787".into())
        .parse()?;
    let explicitly_allows_unauthenticated_ingest =
        std::env::var_os("RLOGS_ALLOW_UNAUTHENTICATED_INGEST").is_some_and(|value| value == "1");
    if !listen.ip().is_loopback()
        && !authentication.is_required()
        && !explicitly_allows_unauthenticated_ingest
    {
        return Err(
            "authenticated ingest is required when the submission service listens outside loopback; configure RLOGS_AUTH_INTROSPECTION_URL (or the temporary RLOGS_INGEST_KEY fallback), or set RLOGS_ALLOW_UNAUTHENTICATED_INGEST=1 only for an intentionally disposable test deployment"
                .into(),
        );
    }

    let service = SubmissionService::open_with_environment_github_archive(
        data_root,
        public_site_url,
        authentication,
    )?;
    if archive_once {
        let repository = service.github_archive_repository().ok_or(
            "--archive-once requires RLOGS_GITHUB_ARCHIVE_REPOSITORY and RLOGS_GITHUB_ARCHIVE_TOKEN",
        )?;
        let archived = service.drain_github_archive_once()?;
        println!("archived {archived} pending evidence package(s) to {repository}");
        return Ok(());
    }
    if let Some(repository) = service.github_archive_repository() {
        println!("private GitHub research archive enabled for {repository}");
        let archive_service = service.clone();
        std::thread::spawn(move || {
            loop {
                if let Err(error) = archive_service.drain_github_archive_once() {
                    eprintln!("GitHub research archive retry remains pending: {error}");
                }
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
        });
    }
    let server_service = service.clone();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    println!("rLogs submission service listening on http://{listen}");
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(listen).await?;
        axum::serve(listener, router(server_service))
            .with_graceful_shutdown(shutdown_signal())
            .await
    })?;
    drop(runtime);
    drop(service);
    Ok(())
}

fn prepare_submission(
    source_path: &PathBuf,
    artifact_path: &PathBuf,
    manifest_path: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    if source_path == artifact_path
        || source_path == manifest_path
        || artifact_path == manifest_path
    {
        return Err("source, artifact, and manifest paths must be distinct".into());
    }

    let artifact_output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(artifact_path)?;
    let export_result = (|| -> Result<_, Box<dyn std::error::Error>> {
        let (mut output, _, privacy_summary) = write_privacy_filtered_submission_log(
            BufReader::new(File::open(source_path)?),
            BufWriter::new(artifact_output),
            RlogLimits::default(),
        )?;
        output.flush()?;
        output.get_ref().sync_all()?;
        drop(output);

        let artifact = build_privacy_verified_submission_artifact(
            File::open(artifact_path)?,
            ArtifactBuildLimits::default(),
            RlogLimits::default(),
        )?;
        let protocol_pack_digest = artifact
            .header
            .region
            .protocol_pack_digest
            .strip_prefix("sha256:")
            .ok_or("artifact protocol-pack digest is missing the sha256: prefix")?;
        let metadata = SubmissionMetadata::new(
            BPSR_GAME_PLUGIN_ID,
            artifact.file_sha256.to_string(),
            artifact.header.schema_version,
            artifact.header.session_id.clone(),
            artifact.header.region.identity.region_id.clone(),
            artifact.header.region.client_build.clone(),
            Sha256Digest::parse(protocol_pack_digest)?,
            submission_privacy_policy_digest(),
            ReportVisibility::Unlisted,
        );
        let manifest = SubmissionSession::new_post_run_artifact(metadata, &artifact)?.manifest();
        let manifest_output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(manifest_path)?;
        let mut manifest_output = BufWriter::new(manifest_output);
        serde_json::to_writer_pretty(&mut manifest_output, &manifest)?;
        manifest_output.write_all(b"\n")?;
        manifest_output.flush()?;
        manifest_output.get_ref().sync_all()?;

        Ok((artifact, privacy_summary))
    })();
    let (artifact, privacy_summary) = match export_result {
        Ok(result) => result,
        Err(error) => {
            let _ = std::fs::remove_file(artifact_path);
            let _ = std::fs::remove_file(manifest_path);
            return Err(error);
        }
    };
    println!(
        "prepared submission: {} bytes, {} canonical events, sha256:{}; excluded {} chat event(s) and stripped {} free-form region evidence value(s)",
        artifact.file_byte_length,
        artifact.rlog.event_count,
        artifact.file_sha256,
        privacy_summary.excluded_chat_events,
        privacy_summary.stripped_region_evidence_entries,
    );
    Ok(())
}

fn contains_network_identifier(value: &str) -> bool {
    value.parse::<IpAddr>().is_ok()
        || value.parse::<SocketAddr>().is_ok()
        || value
            .split(|character: char| {
                !(character.is_ascii_hexdigit() || matches!(character, '.' | ':' | '[' | ']' | '-'))
            })
            .filter(|candidate| !candidate.is_empty())
            .any(|candidate| {
                candidate.parse::<IpAddr>().is_ok()
                    || candidate.parse::<SocketAddr>().is_ok()
                    || looks_like_mac_address(candidate)
            })
}

fn looks_like_mac_address(value: &str) -> bool {
    for separator in [':', '-'] {
        let groups = value.split(separator).collect::<Vec<_>>();
        if groups.len() == 6
            && groups.iter().all(|group| {
                group.len() == 2 && group.chars().all(|value| value.is_ascii_hexdigit())
            })
        {
            return true;
        }
    }
    false
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::contains_network_identifier;

    #[test]
    fn network_identifier_audit_recognizes_addresses_without_flagging_rule_ids() {
        assert!(contains_network_identifier("203.0.113.7"));
        assert!(contains_network_identifier("endpoint=203.0.113.7:443"));
        assert!(contains_network_identifier("[2001:db8::1]:443"));
        assert!(contains_network_identifier("adapter=00-11-22-aa-bb-cc"));
        assert!(!contains_network_identifier(
            "continuous-region:global:asteria"
        ));
    }
}
