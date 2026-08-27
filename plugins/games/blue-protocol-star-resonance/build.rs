use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let seasons_dir = manifest_dir.join("game-data/catalog/dungeon-seasons");
    println!("cargo:rerun-if-changed={}", seasons_dir.display());

    let mut season_paths = fs::read_dir(&seasons_dir)
        .unwrap_or_else(|error| {
            panic!(
                "could not read BPSR dungeon-season catalog {}: {error}",
                seasons_dir.display()
            )
        })
        .map(|entry| {
            entry
                .expect("could not inspect a dungeon-season catalog entry")
                .path()
        })
        .filter(|path| is_season_catalog(path))
        .collect::<Vec<_>>();
    season_paths.sort();
    assert!(
        !season_paths.is_empty(),
        "BPSR must bundle at least one reviewed dungeon-season catalog"
    );

    let mut generated = String::from("const BUNDLED_DUNGEON_SEASONS: &[(&str, &[u8])] = &[\n");
    for path in season_paths {
        println!("cargo:rerun-if-changed={}", path.display());
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("dungeon-season catalog filename must be UTF-8");
        generated.push_str(&format!(
            "    ({file_name:?}, include_bytes!({path:?})),\n",
            path = path.canonicalize().unwrap_or_else(|_| path.clone())
        ));
    }
    generated.push_str("];\n");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out_dir.join("bundled_dungeon_seasons.rs"), generated)
        .expect("could not write generated BPSR dungeon-season registry");
}

fn is_season_catalog(path: &Path) -> bool {
    path.is_file()
        && path.extension().and_then(|extension| extension.to_str()) == Some("json")
        && path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.starts_with("season-"))
}
