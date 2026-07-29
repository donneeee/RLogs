use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rlogs_game_plugin_api::{GAME_PLUGIN_API_VERSION, GamePluginManifest, ResourceStorage};
use rlogs_plugin_api::{OperationStage, PLUGIN_API_VERSION};
use rlogs_plugin_host::{
    PluginPackage, SharedResourceRegistry, discover_plugin_packages, resolve_hook_plan,
    resolve_plugin_load_order,
};
use serde::Serialize;

const INDEX_HTML: &str = include_str!("../static/index.html");
const APP_CSS: &str = include_str!("../static/app.css");
const APP_JS: &str = include_str!("../static/app.js");
const DEFAULT_BIND: &str = "127.0.0.1:7418";

#[derive(Debug)]
struct Options {
    install_root: PathBuf,
    bind: String,
}

#[derive(Debug)]
struct LoadedPackage {
    source: &'static str,
    package: PluginPackage,
}

#[derive(Debug)]
struct LoadedGame {
    folder_name: String,
    root: PathBuf,
    manifest: GamePluginManifest,
}

#[derive(Debug, Serialize)]
struct LabState {
    core: CoreView,
    locations: LocationView,
    summary: SummaryView,
    plugins: Vec<PluginView>,
    resources: Vec<ResourceView>,
    hook_stages: Vec<HookStageView>,
    load_order: Vec<String>,
    issues: Vec<IssueView>,
}

#[derive(Debug, Serialize)]
struct CoreView {
    version: &'static str,
    plugin_api: u16,
    game_plugin_api: u16,
}

#[derive(Debug, Serialize)]
struct LocationView {
    install_root: String,
    installed_plugins: String,
    plugin_assets: String,
    shared_assets: String,
}

#[derive(Debug, Serialize)]
struct SummaryView {
    plugin_count: usize,
    installed_count: usize,
    game_count: usize,
    resource_count: usize,
    issue_count: usize,
}

#[derive(Debug, Serialize)]
struct PluginView {
    id: String,
    name: String,
    version: String,
    folder_name: String,
    source: String,
    runtime: String,
    api_version: u16,
    compatible: bool,
    capabilities: Vec<String>,
    subscriptions: Vec<String>,
    export_count: usize,
    import_count: usize,
    hook_count: usize,
    package_path: String,
    asset_namespace: String,
    shared_asset_namespace: String,
}

#[derive(Debug, Serialize)]
struct ResourceView {
    owner_plugin_id: String,
    name: String,
    kind: String,
    storage: String,
    schema: String,
    path: String,
    exists: bool,
}

#[derive(Debug, Serialize)]
struct HookStageView {
    stage: String,
    before_core: Vec<String>,
    after_core: Vec<String>,
}

#[derive(Debug, Serialize)]
struct IssueView {
    scope: String,
    detail: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Plugin Lab failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_options()?;
    let install_root = fs::canonicalize(&options.install_root)?;
    let address: SocketAddr = options.bind.parse()?;
    if !address.ip().is_loopback() {
        return Err("Plugin Lab only binds to a loopback address".into());
    }
    let listener = TcpListener::bind(address)?;
    println!("rLogs Plugin Lab: http://{address}");
    println!("Install root: {}", install_root.display());
    println!("Press Ctrl+C to stop.");

    for connection in listener.incoming() {
        match connection {
            Ok(mut stream) => {
                if let Err(error) = handle_request(&mut stream, &install_root) {
                    eprintln!("request failed: {error}");
                }
            }
            Err(error) => eprintln!("connection failed: {error}"),
        }
    }
    Ok(())
}

fn parse_options() -> Result<Options, Box<dyn std::error::Error>> {
    const USAGE: &str = "usage: rlogs-plugin-lab [--root <rlogs-install>] [--bind <ip:port>]";
    let mut install_root = std::env::current_dir()?;
    let mut bind = DEFAULT_BIND.to_owned();
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--root") => {
                install_root = arguments
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| format!("--root requires a folder\n{USAGE}"))?;
            }
            Some("--bind") => {
                bind = arguments
                    .next()
                    .and_then(|value| value.into_string().ok())
                    .ok_or_else(|| format!("--bind requires an IP address and port\n{USAGE}"))?;
            }
            Some("-h" | "--help") => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            _ => {
                return Err(
                    format!("unknown argument: {}\n{USAGE}", argument.to_string_lossy()).into(),
                );
            }
        }
    }
    Ok(Options { install_root, bind })
}

fn handle_request(
    stream: &mut TcpStream,
    install_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut request = [0_u8; 16 * 1024];
    let size = stream.read(&mut request)?;
    let request = std::str::from_utf8(&request[..size])?;
    let Some(first_line) = request.lines().next() else {
        return send_response(
            stream,
            "400 Bad Request",
            "text/plain; charset=utf-8",
            b"bad request",
            true,
        );
    };
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let route = parts
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default();
    if method != "GET" {
        return send_response(
            stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"read-only service",
            true,
        );
    }

    match route {
        "/" => send_response(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            INDEX_HTML.as_bytes(),
            false,
        ),
        "/app.css" => send_response(
            stream,
            "200 OK",
            "text/css; charset=utf-8",
            APP_CSS.as_bytes(),
            false,
        ),
        "/app.js" => send_response(
            stream,
            "200 OK",
            "text/javascript; charset=utf-8",
            APP_JS.as_bytes(),
            false,
        ),
        "/api/state" => {
            let body = serde_json::to_vec(&build_state(install_root))?;
            send_response(
                stream,
                "200 OK",
                "application/json; charset=utf-8",
                &body,
                true,
            )
        }
        "/api/health" => send_response(
            stream,
            "200 OK",
            "application/json; charset=utf-8",
            br#"{"status":"ok"}"#,
            true,
        ),
        _ => send_response(
            stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"not found",
            true,
        ),
    }
}

fn send_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    no_store: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let cache = if no_store {
        "Cache-Control: no-store\r\n"
    } else {
        "Cache-Control: public, max-age=300\r\n"
    };
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Connection: close\r\n{cache}X-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; \
         img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes())?;
    stream.write_all(body)?;
    Ok(())
}

fn build_state(install_root: &Path) -> LabState {
    let mut issues = Vec::new();
    let mut ordinary = Vec::new();
    for (source, relative) in [
        ("installed", "plugins/installed"),
        ("built-in", "plugins/builtin/localization"),
        ("example", "plugins/examples"),
    ] {
        let root = install_root.join(relative);
        if !root.is_dir() {
            issues.push(IssueView {
                scope: source.into(),
                detail: format!("missing package folder {}", root.display()),
            });
            continue;
        }
        match discover_plugin_packages(&root, install_root) {
            Ok(report) => {
                issues.extend(report.issues.into_iter().map(|issue| IssueView {
                    scope: source.into(),
                    detail: format!("{}: {}", issue.package_path.display(), issue.detail),
                }));
                ordinary.extend(
                    report
                        .packages
                        .into_iter()
                        .map(|package| LoadedPackage { source, package }),
                );
            }
            Err(error) => issues.push(IssueView {
                scope: source.into(),
                detail: error.to_string(),
            }),
        }
    }
    let games = discover_games(install_root, &mut issues);

    let mut registry = SharedResourceRegistry::default();
    let mut resources = Vec::new();
    for game in &games {
        let plugin_assets = install_root.join("assets").join(&game.folder_name);
        let shared_assets = install_root
            .join("assets")
            .join("shared")
            .join(&game.folder_name);
        if let Err(error) = registry.register_exports_with_asset_roots(
            &game.manifest.id,
            &game.root,
            &plugin_assets,
            &shared_assets,
            &game.manifest.resource_exports,
        ) {
            issues.push(IssueView {
                scope: game.manifest.id.clone(),
                detail: error.to_string(),
            });
        }
        resources.extend(game.manifest.resource_exports.iter().map(|export| {
            resource_view(
                &game.manifest.id,
                export,
                &game.root,
                &plugin_assets,
                &shared_assets,
            )
        }));
    }
    for loaded in &ordinary {
        if let Err(error) = registry.register_package(&loaded.package) {
            issues.push(IssueView {
                scope: loaded.package.manifest().id.clone(),
                detail: error.to_string(),
            });
        }
        resources.extend(
            loaded
                .package
                .manifest()
                .resource_exports
                .iter()
                .map(|export| {
                    resource_view(
                        &loaded.package.manifest().id,
                        export,
                        loaded.package.root(),
                        loaded.package.asset_root(),
                        loaded.package.shared_asset_root(),
                    )
                }),
        );
    }
    for loaded in &ordinary {
        if let Err(error) = registry.validate_imports(&loaded.package) {
            issues.push(IssueView {
                scope: loaded.package.manifest().id.clone(),
                detail: error.to_string(),
            });
        }
    }

    let packages = ordinary
        .iter()
        .map(|loaded| loaded.package.clone())
        .collect::<Vec<_>>();
    let load_order = match resolve_plugin_load_order(&packages) {
        Ok(order) => order,
        Err(error) => {
            issues.push(IssueView {
                scope: "load order".into(),
                detail: error.to_string(),
            });
            Vec::new()
        }
    };
    let hook_stages = OperationStage::ALL
        .into_iter()
        .map(|stage| match resolve_hook_plan(&packages, stage) {
            Ok(plan) => HookStageView {
                stage: enum_name(&stage),
                before_core: plan
                    .before_core
                    .into_iter()
                    .map(|hook| hook.plugin_id)
                    .collect(),
                after_core: plan
                    .after_core
                    .into_iter()
                    .map(|hook| hook.plugin_id)
                    .collect(),
            },
            Err(error) => {
                issues.push(IssueView {
                    scope: enum_name(&stage),
                    detail: error.to_string(),
                });
                HookStageView {
                    stage: enum_name(&stage),
                    before_core: Vec::new(),
                    after_core: Vec::new(),
                }
            }
        })
        .collect::<Vec<_>>();

    let mut plugins = ordinary
        .iter()
        .map(ordinary_plugin_view)
        .chain(
            games
                .iter()
                .map(|game| game_plugin_view(game, install_root)),
        )
        .collect::<Vec<_>>();
    plugins.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then_with(|| left.name.cmp(&right.name))
    });
    resources.sort_by(|left, right| {
        left.owner_plugin_id
            .cmp(&right.owner_plugin_id)
            .then_with(|| left.name.cmp(&right.name))
    });

    let installed_count = ordinary
        .iter()
        .filter(|loaded| loaded.source == "installed")
        .count();
    LabState {
        core: CoreView {
            version: env!("CARGO_PKG_VERSION"),
            plugin_api: PLUGIN_API_VERSION,
            game_plugin_api: GAME_PLUGIN_API_VERSION,
        },
        locations: LocationView {
            install_root: install_root.display().to_string(),
            installed_plugins: install_root.join("plugins/installed").display().to_string(),
            plugin_assets: install_root.join("assets").display().to_string(),
            shared_assets: install_root.join("assets/shared").display().to_string(),
        },
        summary: SummaryView {
            plugin_count: plugins.len(),
            installed_count,
            game_count: games.len(),
            resource_count: resources.len(),
            issue_count: issues.len(),
        },
        plugins,
        resources,
        hook_stages,
        load_order,
        issues,
    }
}

fn discover_games(install_root: &Path, issues: &mut Vec<IssueView>) -> Vec<LoadedGame> {
    let games_root = install_root.join("plugins/games");
    let entries = match fs::read_dir(&games_root) {
        Ok(entries) => entries,
        Err(error) => {
            issues.push(IssueView {
                scope: "games".into(),
                detail: format!("could not read {}: {error}", games_root.display()),
            });
            return Vec::new();
        }
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    paths.sort();
    let mut games = Vec::new();
    for root in paths {
        let Some(folder_name) = root
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            issues.push(IssueView {
                scope: "games".into(),
                detail: format!("non-UTF-8 game plug-in folder {}", root.display()),
            });
            continue;
        };
        let manifest_path = root.join("plugin.toml");
        let manifest = fs::read(&manifest_path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                GamePluginManifest::from_toml(&bytes).map_err(|error| error.to_string())
            });
        match manifest {
            Ok(manifest) => games.push(LoadedGame {
                folder_name,
                root,
                manifest,
            }),
            Err(detail) => issues.push(IssueView {
                scope: folder_name,
                detail: format!("{}: {detail}", manifest_path.display()),
            }),
        }
    }
    games
}

fn ordinary_plugin_view(loaded: &LoadedPackage) -> PluginView {
    let manifest = loaded.package.manifest();
    PluginView {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        folder_name: loaded.package.folder_name().into(),
        source: loaded.source.into(),
        runtime: enum_name(&manifest.runtime),
        api_version: manifest.api_version,
        compatible: manifest.api_version == PLUGIN_API_VERSION,
        capabilities: manifest.capabilities.iter().map(enum_name).collect(),
        subscriptions: manifest.subscriptions.iter().map(enum_name).collect(),
        export_count: manifest.resource_exports.len(),
        import_count: manifest.resource_imports.len(),
        hook_count: manifest.hooks.len(),
        package_path: loaded.package.root().display().to_string(),
        asset_namespace: loaded.package.asset_root().display().to_string(),
        shared_asset_namespace: loaded.package.shared_asset_root().display().to_string(),
    }
}

fn game_plugin_view(game: &LoadedGame, install_root: &Path) -> PluginView {
    PluginView {
        id: game.manifest.id.clone(),
        name: game.manifest.name.clone(),
        version: game.manifest.version.clone(),
        folder_name: game.folder_name.clone(),
        source: "game".into(),
        runtime: enum_name(&game.manifest.runtime),
        api_version: game.manifest.api_version,
        compatible: game.manifest.api_version == GAME_PLUGIN_API_VERSION,
        capabilities: game.manifest.capabilities.iter().map(enum_name).collect(),
        subscriptions: Vec::new(),
        export_count: game.manifest.resource_exports.len(),
        import_count: 0,
        hook_count: 0,
        package_path: game.root.display().to_string(),
        asset_namespace: install_root
            .join("assets")
            .join(&game.folder_name)
            .display()
            .to_string(),
        shared_asset_namespace: install_root
            .join("assets")
            .join("shared")
            .join(&game.folder_name)
            .display()
            .to_string(),
    }
}

fn resource_view(
    owner_plugin_id: &str,
    export: &rlogs_plugin_api::SharedResourceExport,
    package_root: &Path,
    plugin_assets: &Path,
    shared_assets: &Path,
) -> ResourceView {
    let root = match export.storage {
        ResourceStorage::Package => package_root,
        ResourceStorage::PluginAssets => plugin_assets,
        ResourceStorage::SharedAssets => shared_assets,
    };
    let path = root.join(&export.path);
    ResourceView {
        owner_plugin_id: owner_plugin_id.into(),
        name: export.name.clone(),
        kind: export.kind.clone(),
        storage: enum_name(&export.storage),
        schema: format!("{} v{}", export.schema_id, export.schema_version),
        path: path.display().to_string(),
        exists: path.exists(),
    }
}

fn enum_name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_state_exposes_namespaced_shared_game_assets() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let state = build_state(&root);
        assert!(state.plugins.iter().any(|plugin| {
            plugin.id == "app.rlogs.game.blue-protocol-star-resonance" && plugin.source == "game"
        }));
        let icons = state
            .resources
            .iter()
            .find(|resource| resource.name == "icons")
            .expect("BPSR icon resource");
        assert_eq!(icons.storage, "shared_assets");
        assert!(icons.exists);
        assert!(icons.path.contains("assets"));
        assert!(icons.path.contains("shared"));
        assert!(icons.path.contains("blue-protocol-star-resonance"));
    }
}
