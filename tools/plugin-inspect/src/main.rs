use std::ffi::OsString;
use std::path::PathBuf;

use rlogs_plugin_api::OperationStage;
use rlogs_plugin_host::{discover_installed_plugins, resolve_hook_plan, resolve_plugin_load_order};

fn main() {
    if let Err(error) = run() {
        eprintln!("plug-in inspection failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let root = parse_arguments(std::env::args_os().skip(1))?;
    let report = discover_installed_plugins(&root)?;

    println!("plug-in folder: {}", root.display());
    println!("valid packages: {}", report.packages.len());
    for package in &report.packages {
        let manifest = package.manifest();
        println!(
            "- {} {} ({:?})",
            manifest.id, manifest.version, manifest.runtime
        );
        println!("  folder: {}", package.root().display());
        for export in &manifest.resource_exports {
            println!(
                "  exports: {} [{} v{}] -> {}",
                export.name, export.schema_id, export.schema_version, export.path
            );
        }
        for import in &manifest.resource_imports {
            println!(
                "  imports: {}:{}{}",
                import.owner_plugin_id,
                import.name,
                if import.required { " (required)" } else { "" }
            );
        }
    }

    let order = resolve_plugin_load_order(&report.packages)?;
    if order.is_empty() {
        println!("load order: no installed packages");
    } else {
        println!("load order: {}", order.join(" -> "));
    }
    for stage in OperationStage::ALL {
        let plan = resolve_hook_plan(&report.packages, stage)?;
        if plan.before_core.is_empty() && plan.after_core.is_empty() {
            continue;
        }
        println!(
            "{stage:?}: before [{}] | core | after [{}]",
            hook_ids(&plan.before_core),
            hook_ids(&plan.after_core)
        );
    }

    if !report.issues.is_empty() {
        println!("disabled packages: {}", report.issues.len());
        for issue in &report.issues {
            println!("- {}: {}", issue.package_path.display(), issue.detail);
        }
        return Err("one or more plug-in packages failed validation".into());
    }
    Ok(())
}

fn hook_ids(hooks: &[rlogs_plugin_host::ResolvedHook]) -> String {
    hooks
        .iter()
        .map(|hook| hook.plugin_id.as_str())
        .collect::<Vec<_>>()
        .join(" -> ")
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<PathBuf, String> {
    let values: Vec<_> = arguments.into_iter().collect();
    match values.as_slice() {
        [] => Ok(PathBuf::from("plugins/installed")),
        [path] => Ok(PathBuf::from(path)),
        _ => Err("usage: rlogs-plugin-inspect [plugins-folder]".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_folder_is_the_obvious_default() {
        assert_eq!(
            parse_arguments(Vec::<OsString>::new()).unwrap(),
            PathBuf::from("plugins/installed")
        );
    }

    #[test]
    fn accepts_exactly_one_alternate_folder() {
        assert_eq!(
            parse_arguments([OsString::from("plugins/examples")]).unwrap(),
            PathBuf::from("plugins/examples")
        );
        assert!(parse_arguments([OsString::from("a"), OsString::from("b")]).is_err());
    }
}
