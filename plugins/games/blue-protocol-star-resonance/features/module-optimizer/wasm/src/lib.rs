//! Browser adapter for the portable BPSR module optimizer.
//!
//! The WebAssembly boundary deliberately accepts and returns JSON strings.
//! Module instance IDs therefore remain lossless strings, while the optimizer
//! request and response stay identical to the native Plugin Lab contract.

use rlogs_bpsr_module_optimizer::{OptimizeRequest, OptimizerCatalog, ScoringRules, optimize};
use serde::Deserialize;
use wasm_bindgen::prelude::*;

const RUNTIME_CATALOG_JSON: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/optimizer-runtime-catalog.v1.json"
));

#[derive(Debug, Deserialize)]
struct RuntimeCatalog {
    schema_version: u32,
    catalog: OptimizerCatalog,
    link_power: Vec<i32>,
}

/// Returns the exact-build public catalog used to render optimizer controls.
#[wasm_bindgen]
pub fn optimizer_catalog_json() -> Result<String, JsValue> {
    catalog_json_impl().map_err(js_error)
}

/// Runs the same Rust optimization engine used by the native Plugin Lab.
#[wasm_bindgen]
pub fn optimize_json(request_json: &str) -> Result<String, JsValue> {
    optimize_json_impl(request_json).map_err(js_error)
}

fn catalog_json_impl() -> Result<String, String> {
    let runtime = runtime_catalog()?;
    serde_json::to_string(&runtime.catalog)
        .map_err(|error| format!("could not serialize optimizer catalog: {error}"))
}

fn optimize_json_impl(request_json: &str) -> Result<String, String> {
    let runtime = runtime_catalog()?;
    let rules = rules_from_runtime(&runtime)?;
    let request: OptimizeRequest = serde_json::from_str(request_json)
        .map_err(|error| format!("invalid optimizer request: {error}"))?;
    let response = optimize(&rules, &request).map_err(|error| error.to_string())?;
    serde_json::to_string(&response)
        .map_err(|error| format!("could not serialize optimizer response: {error}"))
}

fn runtime_catalog() -> Result<RuntimeCatalog, String> {
    let runtime: RuntimeCatalog = serde_json::from_str(RUNTIME_CATALOG_JSON)
        .map_err(|error| format!("embedded optimizer catalog is invalid: {error}"))?;
    if runtime.schema_version != 1 {
        return Err(format!(
            "unsupported optimizer runtime catalog schema {}",
            runtime.schema_version
        ));
    }
    Ok(runtime)
}

fn rules_from_runtime(runtime: &RuntimeCatalog) -> Result<ScoringRules, String> {
    let rules = ScoringRules::from_catalog_entries(
        runtime.catalog.catalog_revision.clone(),
        &runtime.catalog.attributes,
        runtime.link_power.clone(),
    )
    .map_err(|error| error.to_string())?;
    if rules.scoring_revision() != runtime.catalog.scoring_revision {
        return Err(format!(
            "optimizer scoring revision mismatch: catalog={}, engine={}",
            runtime.catalog.scoring_revision,
            rules.scoring_revision()
        ));
    }
    Ok(rules)
}

fn js_error(message: String) -> JsValue {
    JsValue::from_str(&message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn embedded_catalog_matches_the_current_reviewed_build() {
        let catalog: OptimizerCatalog =
            serde_json::from_str(&catalog_json_impl().unwrap()).unwrap();
        assert_eq!(catalog.attributes.len(), 21);
        assert_eq!(catalog.client_builds, ["24252055"]);
        assert_eq!(catalog.combination_sizes, [4, 5]);
    }

    #[test]
    fn json_bridge_runs_the_real_engine_and_preserves_string_ids() {
        let modules = (0..6)
            .map(|index| {
                json!({
                    "instance_id": format!("900719925474099{}", index * 2 + 3),
                    "config_id": 5_500_101,
                    "quality": 5,
                    "parts": [
                        {"part_id": 1110, "initial_link_points": 4 + index},
                        {"part_id": 1111, "initial_link_points": 3 + index},
                    ],
                })
            })
            .collect::<Vec<_>>();
        let request = json!({
            "modules": modules,
            "target_attributes": [1110],
            "combination_size": 4,
            "max_solutions": 3,
            "search_mode": "exact",
            "require_target_match": false,
        });
        let response: serde_json::Value =
            serde_json::from_str(&optimize_json_impl(&request.to_string()).unwrap()).unwrap();
        assert_eq!(response["solutions"].as_array().unwrap().len(), 3);
        assert!(
            response["solutions"][0]["instance_ids"][0]
                .as_str()
                .unwrap()
                .starts_with("900719925474099")
        );
        assert_eq!(response["search"]["used_mode"], "exact");
    }

    #[test]
    fn website_safe_demo_matches_the_browser_exact_result() {
        let attributes = [
            [1110, 1111],
            [1110, 2104],
            [1111, 1409],
            [1112, 1410],
            [1113, 2105],
            [1114, 2404],
            [1205, 2204],
            [1206, 2205],
            [1307, 2304],
            [1308, 2405],
            [1407, 2406],
            [1408, 1110],
        ];
        let modules = attributes
            .iter()
            .enumerate()
            .map(|(index, parts)| {
                json!({
                    "instance_id": (9_007_199_254_740_993_u64 + index as u64 * 2).to_string(),
                    "config_id": 5_500_101 + index % 4,
                    "quality": 5,
                    "parts": parts
                        .iter()
                        .enumerate()
                        .map(|(part_index, part_id)| json!({
                            "part_id": part_id,
                            "initial_link_points": 3 + (index + part_index * 3) % 8,
                        }))
                        .collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>();
        let request = json!({
            "modules": modules,
            "target_attributes": [1110],
            "combination_size": 4,
            "max_solutions": 10,
            "search_mode": "exact",
            "require_target_match": false,
        });
        let response: serde_json::Value =
            serde_json::from_str(&optimize_json_impl(&request.to_string()).unwrap()).unwrap();

        assert_eq!(response["search"]["total_combinations"], 495);
        assert_eq!(response["search"]["evaluated_states"], 495);
        assert_eq!(response["solutions"][0]["score"], 766);
        assert_eq!(
            response["solutions"][0]["instance_ids"],
            json!([
                "9007199254740993",
                "9007199254740995",
                "9007199254741001",
                "9007199254741015",
            ])
        );
    }
}
