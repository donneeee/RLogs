use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use rlogs_bpsr_module_optimizer::load_catalog_from_path;

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    let catalog_root = manifest_dir.join("../../../game-data/catalog");
    watch_catalog(&catalog_root);

    let (rules, catalog) =
        load_catalog_from_path(&catalog_root).expect("load reviewed optimizer catalog");
    let runtime_catalog = serde_json::json!({
        "schema_version": 1,
        "catalog": catalog,
        "link_power": rules.link_power(),
    });
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"))
        .join("optimizer-runtime-catalog.v1.json");
    fs::write(
        output,
        serde_json::to_vec(&runtime_catalog).expect("serialize optimizer runtime catalog"),
    )
    .expect("write optimizer runtime catalog");
}

fn watch_catalog(root: &Path) {
    println!("cargo:rerun-if-changed={}", root.display());
    for relative in [
        "manifest.json",
        "localization/en-US/modules/profile-catalog.json",
        "module-effects",
        "module-link-effects",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(relative).display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        root.join("../../features/module-optimizer/localization/en-US/attribute-aliases.json")
            .display()
    );
}
