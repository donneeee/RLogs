// Selection errors retain complete exact-build diagnostics for operators; keep
// those values inline instead of boxing every fallible protocol-pack call.
#![allow(clippy::result_large_err)]

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::{MappingProvenance, ProtocolPack, ProtocolPackError};

pub const BPSR_STEAM_APP_ID: &str = "3681810";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveProtocolPackKind {
    Promoted,
    ResearchCandidate,
    CompatibilityFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveProtocolPackSelection {
    pub path: PathBuf,
    /// Build reported by the running Steam client.
    pub build_id: String,
    /// Build for which the selected on-disk pack was generated.
    pub pack_build_id: String,
    pub kind: LiveProtocolPackKind,
}

impl LiveProtocolPackSelection {
    /// Loads the selected pack and, for an explicitly provisional fallback,
    /// retargets a copy in memory to the observed client build. The on-disk
    /// exact-build pack remains immutable and the derived pack carries
    /// provenance that identifies both builds.
    pub fn load_pack(&self) -> Result<ProtocolPack, LiveProtocolPackSelectionError> {
        let bytes = std::fs::read(&self.path).map_err(|source| {
            LiveProtocolPackSelectionError::ReadPack {
                path: self.path.clone(),
                source,
            }
        })?;
        let pack = ProtocolPack::from_json(&bytes).map_err(|source| {
            LiveProtocolPackSelectionError::InvalidPack {
                path: self.path.clone(),
                source,
            }
        })?;
        if self.kind != LiveProtocolPackKind::CompatibilityFallback {
            return Ok(pack);
        }

        let mut definition = pack.definition().clone();
        definition.pack_id = format!(
            "{}-compatibility-fallback-{}",
            definition.pack_id, self.build_id
        );
        definition.target.build_id.clone_from(&self.build_id);
        definition.provenance.push(MappingProvenance {
            source: "provisional-compatible-build-fallback".to_owned(),
            reference: format!(
                "pack_build={};observed_build={}",
                self.pack_build_id, self.build_id
            ),
        });
        ProtocolPack::build(definition).map_err(|source| {
            LiveProtocolPackSelectionError::InvalidPack {
                path: self.path.clone(),
                source,
            }
        })
    }
}

/// Resolves the running Steam build and the best available BPSR protocol pack.
///
/// Exact promoted packs are preferred, followed by exact static candidates.
/// When neither exists, the nearest compatible pack is returned as an explicit
/// provisional fallback. Callers must surface that provenance and preserve
/// undecoded evidence, but may continue the normal parser pipeline.
pub fn resolve_live_steam_protocol_pack(
    plugin_root: &Path,
    executable_path: &Path,
) -> Result<LiveProtocolPackSelection, LiveProtocolPackSelectionError> {
    let manifest_path = steam_manifest_for_executable(executable_path)?;
    let manifest = std::fs::read_to_string(&manifest_path).map_err(|source| {
        LiveProtocolPackSelectionError::ReadManifest {
            path: manifest_path.clone(),
            source,
        }
    })?;
    let app_id = acf_value(&manifest, "appid").ok_or_else(|| {
        LiveProtocolPackSelectionError::MissingManifestValue {
            path: manifest_path.clone(),
            key: "appid",
        }
    })?;
    if app_id != BPSR_STEAM_APP_ID {
        return Err(LiveProtocolPackSelectionError::UnexpectedAppId {
            expected: BPSR_STEAM_APP_ID,
            actual: app_id,
        });
    }
    let build_id = acf_value(&manifest, "buildid").ok_or({
        LiveProtocolPackSelectionError::MissingManifestValue {
            path: manifest_path,
            key: "buildid",
        }
    })?;
    validate_build_id(&build_id)?;

    let promoted = plugin_root
        .join("protocol-packs/global")
        .join(format!("steam-{build_id}"))
        .join("pack.json");
    if promoted.is_file() {
        validate_exact_pack(&promoted, &build_id)?;
        return Ok(LiveProtocolPackSelection {
            path: promoted,
            pack_build_id: build_id.clone(),
            build_id,
            kind: LiveProtocolPackKind::Promoted,
        });
    }

    let candidate = plugin_root
        .join("research/game-file-inventory/global")
        .join(format!("steam-{build_id}"))
        .join("protocol-pack-static-candidate.v2.json");
    if candidate.is_file() {
        validate_exact_pack(&candidate, &build_id)?;
        return Ok(LiveProtocolPackSelection {
            path: candidate,
            pack_build_id: build_id.clone(),
            build_id,
            kind: LiveProtocolPackKind::ResearchCandidate,
        });
    }

    if let Some((path, pack_build_id)) = latest_compatible_pack(plugin_root, &build_id)? {
        return Ok(LiveProtocolPackSelection {
            path,
            build_id,
            pack_build_id,
            kind: LiveProtocolPackKind::CompatibilityFallback,
        });
    }

    Err(LiveProtocolPackSelectionError::NoExactPack {
        build_id,
        promoted,
        candidate,
    })
}

fn latest_compatible_pack(
    plugin_root: &Path,
    observed_build_id: &str,
) -> Result<Option<(PathBuf, String)>, LiveProtocolPackSelectionError> {
    let observed = observed_build_id.parse::<u64>().map_err(|_| {
        LiveProtocolPackSelectionError::InvalidBuildId(observed_build_id.to_owned())
    })?;
    let roots = [
        (
            plugin_root.join("protocol-packs/global"),
            PathBuf::from("pack.json"),
            1_u8,
        ),
        (
            plugin_root.join("research/game-file-inventory/global"),
            PathBuf::from("protocol-pack-static-candidate.v2.json"),
            0_u8,
        ),
    ];
    let mut candidates = Vec::new();
    for (root, filename, promoted_priority) in roots {
        let entries = match std::fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(LiveProtocolPackSelectionError::ReadPackDirectory {
                    path: root,
                    source,
                });
            }
        };
        for entry in entries {
            let entry =
                entry.map_err(|source| LiveProtocolPackSelectionError::ReadPackDirectory {
                    path: root.clone(),
                    source,
                })?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(value) = name.strip_prefix("steam-") else {
                continue;
            };
            let Ok(numeric_build) = value.parse::<u64>() else {
                continue;
            };
            if numeric_build == observed {
                continue;
            }
            let path = entry.path().join(&filename);
            if path.is_file() {
                candidates.push((numeric_build, promoted_priority, path, value.to_owned()));
            }
        }
    }
    candidates.sort_by_key(|(build, priority, _, _)| {
        let newer_than_observed = *build > observed;
        let distance = build.abs_diff(observed);
        (newer_than_observed, distance, std::cmp::Reverse(*priority))
    });
    let Some((_, _, path, pack_build_id)) = candidates.into_iter().next() else {
        return Ok(None);
    };
    validate_exact_pack(&path, &pack_build_id)?;
    Ok(Some((path, pack_build_id)))
}

pub fn steam_manifest_for_executable(
    executable_path: &Path,
) -> Result<PathBuf, LiveProtocolPackSelectionError> {
    for ancestor in executable_path.ancestors() {
        if ancestor
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("common"))
        {
            let steamapps = ancestor.parent().ok_or_else(|| {
                LiveProtocolPackSelectionError::NoSteamManifest(executable_path.to_path_buf())
            })?;
            return Ok(steamapps.join(format!("appmanifest_{BPSR_STEAM_APP_ID}.acf")));
        }
    }
    Err(LiveProtocolPackSelectionError::NoSteamManifest(
        executable_path.to_path_buf(),
    ))
}

fn validate_exact_pack(path: &Path, build_id: &str) -> Result<(), LiveProtocolPackSelectionError> {
    let bytes = std::fs::read(path).map_err(|source| LiveProtocolPackSelectionError::ReadPack {
        path: path.to_path_buf(),
        source,
    })?;
    let pack = ProtocolPack::from_json(&bytes).map_err(|source| {
        LiveProtocolPackSelectionError::InvalidPack {
            path: path.to_path_buf(),
            source,
        }
    })?;
    let target = &pack.definition().target;
    if target.deployment_id != "global" || target.channel != "steam" || target.build_id != build_id
    {
        return Err(LiveProtocolPackSelectionError::PackTargetMismatch {
            path: path.to_path_buf(),
            expected_build: build_id.to_owned(),
            actual_deployment: target.deployment_id.clone(),
            actual_channel: target.channel.clone(),
            actual_build: target.build_id.clone(),
        });
    }
    Ok(())
}

fn acf_value(input: &str, key: &str) -> Option<String> {
    input.lines().find_map(|line| {
        let mut values = line.split('"').skip(1).step_by(2);
        let candidate = values.next()?;
        let value = values.next()?;
        candidate
            .eq_ignore_ascii_case(key)
            .then(|| value.to_owned())
    })
}

fn validate_build_id(value: &str) -> Result<(), LiveProtocolPackSelectionError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(LiveProtocolPackSelectionError::InvalidBuildId(
            value.to_owned(),
        ));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum LiveProtocolPackSelectionError {
    #[error("could not locate the Steam app manifest from game executable {0}")]
    NoSteamManifest(PathBuf),

    #[error("could not read Steam manifest {path}: {source}")]
    ReadManifest {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Steam manifest {path} has no {key} value")]
    MissingManifestValue { path: PathBuf, key: &'static str },

    #[error("Steam manifest belongs to app {actual}, expected {expected}")]
    UnexpectedAppId {
        expected: &'static str,
        actual: String,
    },

    #[error("Steam manifest contains invalid build id {0:?}")]
    InvalidBuildId(String),

    #[error(
        "no exact BPSR pack exists for Steam build {build_id}; checked promoted {promoted} and research candidate {candidate}",
        promoted = promoted.display(),
        candidate = candidate.display()
    )]
    NoExactPack {
        build_id: String,
        promoted: PathBuf,
        candidate: PathBuf,
    },

    #[error("could not read protocol pack {path}: {source}")]
    ReadPack {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("could not inspect protocol pack directory {path}: {source}")]
    ReadPackDirectory {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("protocol pack {path} is invalid: {source}")]
    InvalidPack {
        path: PathBuf,
        source: ProtocolPackError,
    },

    #[error(
        "protocol pack {path} targets {actual_deployment}/{actual_channel}/{actual_build}, expected global/steam/{expected_build}",
        path = path.display()
    )]
    PackTargetMismatch {
        path: PathBuf,
        expected_build: String,
        actual_deployment: String,
        actual_channel: String,
        actual_build: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_manifest_next_to_any_steam_library_common_folder() {
        let executable = Path::new(
            r"G:\SteamLibrary\steamapps\common\Blue Protocol Star Resonance\BPSR_STEAM.exe",
        );
        assert_eq!(
            steam_manifest_for_executable(executable).unwrap(),
            PathBuf::from(r"G:\SteamLibrary\steamapps\appmanifest_3681810.acf")
        );
    }

    #[test]
    fn parses_quoted_acf_values_without_accepting_partial_keys() {
        let manifest = r#"
            "appid" "3681810"
            "buildid_old" "1"
            "buildid" "24609362"
        "#;
        assert_eq!(acf_value(manifest, "appid").as_deref(), Some("3681810"));
        assert_eq!(acf_value(manifest, "buildid").as_deref(), Some("24609362"));
    }

    #[test]
    fn exact_candidate_is_selected_until_a_promoted_pack_exists() {
        let root = std::env::temp_dir().join(format!(
            "rlogs-bpsr-live-pack-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let steamapps = root.join("steamapps");
        let executable = steamapps.join("common/BPSR/bpsr/BPSR_STEAM.exe");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(
            steamapps.join("appmanifest_3681810.acf"),
            r#""appid" "3681810"
"buildid" "24687926""#,
        )
        .unwrap();

        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("protocol-packs/global/steam-24687926/pack.json");
        let plugin_root = root.join("plugin");
        let candidate = plugin_root.join(
            "research/game-file-inventory/global/steam-24687926/protocol-pack-static-candidate.v2.json",
        );
        std::fs::create_dir_all(candidate.parent().unwrap()).unwrap();
        std::fs::copy(&source, &candidate).unwrap();

        let selected = resolve_live_steam_protocol_pack(&plugin_root, &executable).unwrap();
        assert_eq!(selected.kind, LiveProtocolPackKind::ResearchCandidate);
        assert_eq!(selected.build_id, "24687926");
        assert_eq!(selected.pack_build_id, "24687926");
        assert_eq!(selected.path, candidate);

        let promoted = plugin_root.join("protocol-packs/global/steam-24687926/pack.json");
        std::fs::create_dir_all(promoted.parent().unwrap()).unwrap();
        std::fs::copy(source, &promoted).unwrap();
        let selected = resolve_live_steam_protocol_pack(&plugin_root, &executable).unwrap();
        assert_eq!(selected.kind, LiveProtocolPackKind::Promoted);
        assert_eq!(selected.pack_build_id, "24687926");
        assert_eq!(selected.path, promoted);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn newest_compatible_pack_is_retargeted_provisionally_for_a_new_build() {
        let root = std::env::temp_dir().join(format!(
            "rlogs-bpsr-live-pack-fallback-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let steamapps = root.join("steamapps");
        let executable = steamapps.join("common/BPSR/bpsr/BPSR_STEAM.exe");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(
            steamapps.join("appmanifest_3681810.acf"),
            r#""appid" "3681810"
"buildid" "24699999""#,
        )
        .unwrap();

        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("protocol-packs/global/steam-24687926/pack.json");
        let plugin_root = root.join("plugin");
        let candidate = plugin_root.join(
            "research/game-file-inventory/global/steam-24687926/protocol-pack-static-candidate.v2.json",
        );
        std::fs::create_dir_all(candidate.parent().unwrap()).unwrap();
        std::fs::copy(source, &candidate).unwrap();

        let selected = resolve_live_steam_protocol_pack(&plugin_root, &executable).unwrap();
        assert_eq!(selected.kind, LiveProtocolPackKind::CompatibilityFallback);
        assert_eq!(selected.build_id, "24699999");
        assert_eq!(selected.pack_build_id, "24687926");
        assert_eq!(selected.path, candidate);

        let loaded = selected.load_pack().unwrap();
        assert_eq!(loaded.definition().target.build_id, "24699999");
        assert!(
            loaded
                .definition()
                .pack_id
                .contains("compatibility-fallback")
        );
        assert!(loaded.definition().provenance.iter().any(|entry| {
            entry.source == "provisional-compatible-build-fallback"
                && entry.reference == "pack_build=24687926;observed_build=24699999"
        }));

        std::fs::remove_dir_all(root).unwrap();
    }
}
