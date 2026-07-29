use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const SCHEMA_VERSION: u16 = 1;

fn main() {
    if let Err(error) = run() {
        eprintln!("reference reconciliation failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), ReconcileError> {
    let arguments = arguments()?;
    let observation: Observation = serde_json::from_slice(&std::fs::read(&arguments.observation)?)?;
    let manifests = arguments
        .manifests
        .iter()
        .map(|path| serde_json::from_slice(&std::fs::read(path)?).map_err(ReconcileError::from))
        .collect::<Result<Vec<ReferenceManifest>, ReconcileError>>()?;
    let report = reconcile(observation, manifests)?;

    if arguments.json {
        serde_json::to_writer_pretty(std::io::stdout().lock(), &report)?;
        println!();
    } else {
        print_text(&report);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Direction {
    ClientToServer,
    ServerToClient,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Fragment {
    Call,
    Notify,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    direction: Direction,
    fragment: Fragment,
    service_id: u64,
    method_id: u32,
}

#[derive(Debug, Deserialize)]
struct Observation {
    schema_version: u16,
    observation_id: String,
    observed_routed_packets: Vec<ObservedRoute>,
}

#[derive(Debug, Deserialize)]
struct ObservedRoute {
    direction: Direction,
    fragment: Fragment,
    service_id: u64,
    method_id: u32,
    packet_count: u64,
}

impl ObservedRoute {
    fn route(&self) -> Route {
        Route {
            direction: self.direction.clone(),
            fragment: self.fragment.clone(),
            service_id: self.service_id,
            method_id: self.method_id,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceManifest {
    schema_version: u16,
    reference_id: String,
    lineage_id: String,
    project: String,
    revision: String,
    source_urls: Vec<String>,
    claims: Vec<RouteClaim>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteClaim {
    route: Route,
    service_name: String,
    method_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct Mapping {
    service_name: String,
    method_name: String,
}

impl From<&RouteClaim> for Mapping {
    fn from(claim: &RouteClaim) -> Self {
        Self {
            service_name: claim.service_name.clone(),
            method_name: claim.method_name.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReconciliationStatus {
    Corroborated,
    SingleLineage,
    Conflict,
    Unmapped,
}

#[derive(Debug, Serialize)]
struct ReconciliationReport {
    schema_version: u16,
    observation_id: String,
    reference_ids: Vec<String>,
    summary: ReconciliationSummary,
    routes: Vec<ReconciledRoute>,
}

#[derive(Debug, Default, Serialize)]
struct ReconciliationSummary {
    observed_routes: usize,
    corroborated: usize,
    single_lineage: usize,
    conflicts: usize,
    unmapped: usize,
}

#[derive(Debug, Serialize)]
struct ReconciledRoute {
    route: Route,
    packet_count: u64,
    status: ReconciliationStatus,
    candidate: Option<Mapping>,
    independent_lineages: usize,
    claims: Vec<ReportClaim>,
}

#[derive(Debug, Serialize)]
struct ReportClaim {
    reference_id: String,
    lineage_id: String,
    project: String,
    revision: String,
    source_urls: Vec<String>,
    service_name: String,
    method_name: String,
}

fn reconcile(
    observation: Observation,
    mut manifests: Vec<ReferenceManifest>,
) -> Result<ReconciliationReport, ReconcileError> {
    if observation.schema_version != SCHEMA_VERSION {
        return Err(ReconcileError::UnsupportedObservationSchema(
            observation.schema_version,
        ));
    }
    let mut observed = BTreeMap::new();
    for entry in observation.observed_routed_packets {
        let route = entry.route();
        if observed.insert(route.clone(), entry.packet_count).is_some() {
            return Err(ReconcileError::DuplicateObservedRoute(route));
        }
    }

    manifests.sort_by(|left, right| left.reference_id.cmp(&right.reference_id));
    let mut reference_ids = BTreeSet::new();
    let mut claims_by_route: BTreeMap<Route, Vec<ReportClaim>> = BTreeMap::new();
    for manifest in manifests {
        validate_manifest(&manifest, &mut reference_ids)?;
        for claim in manifest.claims {
            if !observed.contains_key(&claim.route) {
                continue;
            }
            claims_by_route
                .entry(claim.route)
                .or_default()
                .push(ReportClaim {
                    reference_id: manifest.reference_id.clone(),
                    lineage_id: manifest.lineage_id.clone(),
                    project: manifest.project.clone(),
                    revision: manifest.revision.clone(),
                    source_urls: manifest.source_urls.clone(),
                    service_name: claim.service_name,
                    method_name: claim.method_name,
                });
        }
    }

    let mut summary = ReconciliationSummary {
        observed_routes: observed.len(),
        ..ReconciliationSummary::default()
    };
    let mut routes = Vec::with_capacity(observed.len());
    for (route, packet_count) in observed {
        let claims = claims_by_route.remove(&route).unwrap_or_default();
        let mut lineage_mappings: BTreeMap<String, BTreeSet<Mapping>> = BTreeMap::new();
        for claim in &claims {
            lineage_mappings
                .entry(claim.lineage_id.clone())
                .or_default()
                .insert(Mapping {
                    service_name: claim.service_name.clone(),
                    method_name: claim.method_name.clone(),
                });
        }

        let has_internal_lineage_conflict =
            lineage_mappings.values().any(|mappings| mappings.len() > 1);
        let votes = lineage_mappings
            .values()
            .filter_map(|mappings| {
                (mappings.len() == 1)
                    .then(|| mappings.first().cloned())
                    .flatten()
            })
            .fold(BTreeMap::<Mapping, usize>::new(), |mut counts, mapping| {
                *counts.entry(mapping).or_default() += 1;
                counts
            });

        let (status, candidate) = if claims.is_empty() {
            (ReconciliationStatus::Unmapped, None)
        } else if has_internal_lineage_conflict || votes.len() > 1 {
            (ReconciliationStatus::Conflict, None)
        } else {
            let (candidate, independent_votes) = votes
                .first_key_value()
                .expect("claims created one lineage vote");
            if *independent_votes >= 2 {
                (ReconciliationStatus::Corroborated, Some(candidate.clone()))
            } else {
                (ReconciliationStatus::SingleLineage, Some(candidate.clone()))
            }
        };
        match status {
            ReconciliationStatus::Corroborated => summary.corroborated += 1,
            ReconciliationStatus::SingleLineage => summary.single_lineage += 1,
            ReconciliationStatus::Conflict => summary.conflicts += 1,
            ReconciliationStatus::Unmapped => summary.unmapped += 1,
        }
        routes.push(ReconciledRoute {
            route,
            packet_count,
            status,
            candidate,
            independent_lineages: lineage_mappings.len(),
            claims,
        });
    }

    Ok(ReconciliationReport {
        schema_version: SCHEMA_VERSION,
        observation_id: observation.observation_id,
        reference_ids: reference_ids.into_iter().collect(),
        summary,
        routes,
    })
}

fn validate_manifest(
    manifest: &ReferenceManifest,
    reference_ids: &mut BTreeSet<String>,
) -> Result<(), ReconcileError> {
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(ReconcileError::UnsupportedManifestSchema {
            reference_id: manifest.reference_id.clone(),
            version: manifest.schema_version,
        });
    }
    if !reference_ids.insert(manifest.reference_id.clone()) {
        return Err(ReconcileError::DuplicateReferenceId(
            manifest.reference_id.clone(),
        ));
    }
    if manifest.lineage_id.trim().is_empty()
        || manifest.project.trim().is_empty()
        || manifest.revision.trim().is_empty()
        || manifest.source_urls.is_empty()
    {
        return Err(ReconcileError::IncompleteReference(
            manifest.reference_id.clone(),
        ));
    }
    let mut routes = BTreeSet::new();
    for claim in &manifest.claims {
        if claim.service_name.trim().is_empty() || claim.method_name.trim().is_empty() {
            return Err(ReconcileError::EmptyMapping {
                reference_id: manifest.reference_id.clone(),
                route: claim.route.clone(),
            });
        }
        let computed_service_id = service_id(&claim.service_name);
        if computed_service_id != claim.route.service_id {
            return Err(ReconcileError::ServiceIdMismatch {
                reference_id: manifest.reference_id.clone(),
                service_name: claim.service_name.clone(),
                declared: claim.route.service_id,
                computed: computed_service_id,
            });
        }
        if !routes.insert(claim.route.clone()) {
            return Err(ReconcileError::DuplicateManifestRoute {
                reference_id: manifest.reference_id.clone(),
                route: claim.route.clone(),
            });
        }
    }
    Ok(())
}

fn service_id(service_name: &str) -> u64 {
    u64::from(
        service_name.bytes().fold(0_u32, |hash, byte| {
            hash.wrapping_mul(131).wrapping_add(u32::from(byte))
        }) & 0x7fff_ffff,
    )
}

fn print_text(report: &ReconciliationReport) {
    println!("observation: {}", report.observation_id);
    println!(
        "routes: {} observed, {} corroborated, {} single-lineage, {} conflict, {} unmapped",
        report.summary.observed_routes,
        report.summary.corroborated,
        report.summary.single_lineage,
        report.summary.conflicts,
        report.summary.unmapped
    );
    for route in &report.routes {
        let candidate = route
            .candidate
            .as_ref()
            .map(|mapping| format!("{}/{}", mapping.service_name, mapping.method_name))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:?} {:?} {}/{} packets={} {:?} candidate={}",
            route.route.direction,
            route.route.fragment,
            route.route.service_id,
            route.route.method_id,
            route.packet_count,
            route.status,
            candidate
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Arguments {
    observation: PathBuf,
    manifests: Vec<PathBuf>,
    json: bool,
}

fn arguments() -> Result<Arguments, ReconcileError> {
    parse_arguments(std::env::args_os().skip(1))
}

fn parse_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Arguments, ReconcileError> {
    let mut observation = None;
    let mut manifests = Vec::new();
    let mut json = false;
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        if argument == OsStr::new("--observation") {
            if observation.is_some() {
                return Err(ReconcileError::Usage);
            }
            observation = arguments.next().map(PathBuf::from);
        } else if argument == OsStr::new("--json") {
            if json {
                return Err(ReconcileError::Usage);
            }
            json = true;
        } else if argument.to_string_lossy().starts_with('-') {
            return Err(ReconcileError::Usage);
        } else {
            manifests.push(PathBuf::from(argument));
        }
    }
    if manifests.is_empty() {
        return Err(ReconcileError::Usage);
    }
    Ok(Arguments {
        observation: observation.ok_or(ReconcileError::Usage)?,
        manifests,
        json,
    })
}

#[derive(Debug, Error)]
enum ReconcileError {
    #[error(
        "usage: rlogs-reference-reconcile --observation <observation.json> [--json] <manifest.json>..."
    )]
    Usage,
    #[error("unsupported observation schema version {0}")]
    UnsupportedObservationSchema(u16),
    #[error("reference {reference_id} uses unsupported schema version {version}")]
    UnsupportedManifestSchema { reference_id: String, version: u16 },
    #[error("duplicate reference ID {0}")]
    DuplicateReferenceId(String),
    #[error("reference {0} is missing lineage, project, revision, or source URL evidence")]
    IncompleteReference(String),
    #[error("observation repeats route {0:?}")]
    DuplicateObservedRoute(Route),
    #[error("reference {reference_id} repeats route {route:?}")]
    DuplicateManifestRoute { reference_id: String, route: Route },
    #[error("reference {reference_id} has an empty mapping for route {route:?}")]
    EmptyMapping { reference_id: String, route: Route },
    #[error(
        "reference {reference_id} maps service {service_name} to {declared}, but its protocol hash is {computed}"
    )]
    ServiceIdMismatch {
        reference_id: String,
        service_name: String,
        declared: u64,
        computed: u64,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(method_id: u32) -> Route {
        Route {
            direction: Direction::ServerToClient,
            fragment: Fragment::Notify,
            service_id: service_id("WorldNtf"),
            method_id,
        }
    }

    fn observation(method_ids: &[u32]) -> Observation {
        Observation {
            schema_version: SCHEMA_VERSION,
            observation_id: "test".into(),
            observed_routed_packets: method_ids
                .iter()
                .map(|method_id| ObservedRoute {
                    direction: Direction::ServerToClient,
                    fragment: Fragment::Notify,
                    service_id: service_id("WorldNtf"),
                    method_id: *method_id,
                    packet_count: 1,
                })
                .collect(),
        }
    }

    fn manifest(reference_id: &str, lineage_id: &str, claims: &[(u32, &str)]) -> ReferenceManifest {
        ReferenceManifest {
            schema_version: SCHEMA_VERSION,
            reference_id: reference_id.into(),
            lineage_id: lineage_id.into(),
            project: reference_id.into(),
            revision: "immutable".into(),
            source_urls: vec!["https://example.invalid/source".into()],
            claims: claims
                .iter()
                .map(|(method_id, method_name)| RouteClaim {
                    route: route(*method_id),
                    service_name: "WorldNtf".into(),
                    method_name: (*method_name).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn related_forks_are_one_vote_but_independent_agreement_is_corroborated() {
        let report = reconcile(
            observation(&[1]),
            vec![
                manifest("parent", "lineage-a", &[(1, "Mapped")]),
                manifest("fork", "lineage-a", &[(1, "Mapped")]),
                manifest("independent", "lineage-b", &[(1, "Mapped")]),
            ],
        )
        .unwrap();

        assert_eq!(report.routes[0].status, ReconciliationStatus::Corroborated);
        assert_eq!(report.routes[0].independent_lineages, 2);
        assert_eq!(report.routes[0].claims.len(), 3);
    }

    #[test]
    fn conflicts_and_missing_routes_are_preserved() {
        let report = reconcile(
            observation(&[1, 2]),
            vec![
                manifest("one", "lineage-a", &[(1, "First")]),
                manifest("two", "lineage-b", &[(1, "Second")]),
            ],
        )
        .unwrap();

        assert_eq!(report.routes[0].status, ReconciliationStatus::Conflict);
        assert!(report.routes[0].candidate.is_none());
        assert_eq!(report.routes[1].status, ReconciliationStatus::Unmapped);
    }

    #[test]
    fn duplicate_observation_routes_are_rejected() {
        assert!(matches!(
            reconcile(observation(&[1, 1]), Vec::new()),
            Err(ReconcileError::DuplicateObservedRoute(_))
        ));
    }

    #[test]
    fn arguments_require_an_observation_and_at_least_one_manifest() {
        assert!(parse_arguments([OsString::from("--json")]).is_err());
        assert!(
            parse_arguments([
                OsString::from("--observation"),
                OsString::from("observed.json"),
                OsString::from("manifest.json"),
            ])
            .is_ok()
        );
    }

    #[test]
    fn service_id_uses_the_protocol_bkdr_131_hash() {
        assert_eq!(service_id("WorldNtf"), 1_664_308_034);
        assert_eq!(service_id("World"), 103_198_054);
        assert_eq!(service_id("Ace"), 1_128_535);
        assert_eq!(service_id("ChitChat"), 1_321_197_368);
    }

    #[test]
    fn manifest_rejects_a_service_name_that_does_not_match_its_route() {
        let mut reference_ids = BTreeSet::new();
        let mut bad = manifest("bad", "lineage", &[(1, "Mapped")]);
        bad.claims[0].service_name = "WrongService".into();

        assert!(matches!(
            validate_manifest(&bad, &mut reference_ids),
            Err(ReconcileError::ServiceIdMismatch { .. })
        ));
    }
}
