use std::fs;
use std::io::{BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rlogs_bpsr_module_optimizer::{
    OptimizeRequest, OptimizeResponse, OptimizerCatalog, load_catalog_from_install_root, optimize,
};
use rlogs_game_plugin_api::{GAME_PLUGIN_API_VERSION, GamePluginManifest, ResourceStorage};
use rlogs_log_format::RlogLimits;
use rlogs_plugin_api::{OperationStage, PLUGIN_API_VERSION};
use rlogs_plugin_combat_meter::CombatTimelinePlugin;
use rlogs_plugin_host::{
    PluginPackage, SharedResourceRegistry, discover_plugin_packages, resolve_hook_plan,
    resolve_plugin_load_order,
};
use rlogs_plugin_runtime::{PluginRunLimits, PluginRunReport, ReplayPlugin, replay_rlog};
use serde::{Deserialize, Serialize};

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
    fixtures: Vec<FixtureView>,
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

#[derive(Debug, Serialize)]
struct FixtureView {
    file_name: String,
    display_name: String,
    bytes: u64,
}

#[derive(Debug, Deserialize)]
struct ReplayRequest {
    fixture: String,
}

#[derive(Debug, Serialize)]
struct ReplayResponse {
    fixture: String,
    report: PluginRunReport,
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    route: String,
    body: Vec<u8>,
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
    let request = read_http_request(stream)?;
    if request.method != "GET" && request.method != "POST" {
        return send_response(
            stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"read-only service",
            true,
        );
    }

    match (request.method.as_str(), request.route.as_str()) {
        ("GET", "/") => send_response(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            INDEX_HTML.as_bytes(),
            false,
        ),
        ("GET", "/app.css") => send_response(
            stream,
            "200 OK",
            "text/css; charset=utf-8",
            APP_CSS.as_bytes(),
            false,
        ),
        ("GET", "/app.js") => send_response(
            stream,
            "200 OK",
            "text/javascript; charset=utf-8",
            APP_JS.as_bytes(),
            false,
        ),
        ("GET", "/api/state") => {
            let body = serde_json::to_vec(&build_state(install_root))?;
            send_response(
                stream,
                "200 OK",
                "application/json; charset=utf-8",
                &body,
                true,
            )
        }
        ("GET", "/api/health") => send_response(
            stream,
            "200 OK",
            "application/json; charset=utf-8",
            br#"{"status":"ok"}"#,
            true,
        ),
        ("GET", "/api/module-optimizer/catalog") => match load_optimizer_catalog(install_root) {
            Ok(catalog) => {
                let body = serde_json::to_vec(&catalog)?;
                send_response(
                    stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    &body,
                    true,
                )
            }
            Err(detail) => send_json_error(stream, "500 Internal Server Error", &detail),
        },
        ("POST", "/api/module-optimizer/optimize") => {
            match run_module_optimizer(install_root, &request.body) {
                Ok(result) => {
                    let body = serde_json::to_vec(&result)?;
                    send_response(
                        stream,
                        "200 OK",
                        "application/json; charset=utf-8",
                        &body,
                        true,
                    )
                }
                Err(detail) => send_json_error(stream, "400 Bad Request", &detail),
            }
        }
        ("POST", "/api/replay") => match run_replay(install_root, &request.body) {
            Ok(result) => {
                let body = serde_json::to_vec(&result)?;
                send_response(
                    stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    &body,
                    true,
                )
            }
            Err(detail) => {
                let body = serde_json::to_vec(&serde_json::json!({ "error": detail }))?;
                send_response(
                    stream,
                    "400 Bad Request",
                    "application/json; charset=utf-8",
                    &body,
                    true,
                )
            }
        },
        _ => send_response(
            stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"not found",
            true,
        ),
    }
}

fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, Box<dyn std::error::Error>> {
    const MAXIMUM_HEADER_BYTES: usize = 16 * 1024;
    const MAXIMUM_BODY_BYTES: usize = 2 * 1024 * 1024;
    let mut bytes = Vec::with_capacity(4 * 1024);
    let header_end = loop {
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        if bytes.len() >= MAXIMUM_HEADER_BYTES {
            return Err("HTTP headers exceed the local API limit".into());
        }
        let mut chunk = [0_u8; 4 * 1024];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err("incomplete HTTP request".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
    };
    let header = std::str::from_utf8(&bytes[..header_end])?;
    let mut lines = header.lines();
    let first_line = lines.next().ok_or("missing HTTP request line")?;
    let mut parts = first_line.split_whitespace();
    let method = parts.next().ok_or("missing HTTP method")?.to_owned();
    let route = parts
        .next()
        .ok_or("missing HTTP route")?
        .split('?')
        .next()
        .unwrap_or_default()
        .to_owned();
    let mut content_length = 0_usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse()?;
        }
    }
    if content_length > MAXIMUM_BODY_BYTES {
        return Err("HTTP request body exceeds the local API limit".into());
    }
    let required = header_end
        .checked_add(content_length)
        .ok_or("HTTP request length overflow")?;
    while bytes.len() < required {
        let mut chunk = [0_u8; 4 * 1024];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err("incomplete HTTP request body".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > MAXIMUM_HEADER_BYTES + MAXIMUM_BODY_BYTES {
            return Err("HTTP request exceeds the local API limit".into());
        }
    }
    Ok(HttpRequest {
        method,
        route,
        body: bytes[header_end..required].to_vec(),
    })
}

fn load_optimizer_catalog(install_root: &Path) -> Result<OptimizerCatalog, String> {
    load_catalog_from_install_root(install_root)
        .map(|(_, catalog)| catalog)
        .map_err(|error| error.to_string())
}

fn run_module_optimizer(install_root: &Path, body: &[u8]) -> Result<OptimizeResponse, String> {
    let request: OptimizeRequest = serde_json::from_slice(body)
        .map_err(|error| format!("invalid module optimizer request: {error}"))?;
    let (rules, _) =
        load_catalog_from_install_root(install_root).map_err(|error| error.to_string())?;
    optimize(&rules, &request).map_err(|error| error.to_string())
}

fn run_replay(install_root: &Path, body: &[u8]) -> Result<ReplayResponse, String> {
    let request: ReplayRequest =
        serde_json::from_slice(body).map_err(|error| format!("invalid replay request: {error}"))?;
    validate_fixture_name(&request.fixture)?;
    let path = install_root
        .join("tests/fixtures/replay")
        .join(&request.fixture);
    let input = fs::File::open(&path)
        .map_err(|error| format!("could not open fixture {}: {error}", path.display()))?;
    let report = replay_rlog(
        BufReader::new(input),
        CombatTimelinePlugin::new(),
        RlogLimits::default(),
        PluginRunLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    Ok(ReplayResponse {
        fixture: request.fixture,
        report,
    })
}

fn send_json_error(
    stream: &mut TcpStream,
    status: &str,
    detail: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let body = serde_json::to_vec(&serde_json::json!({ "error": detail }))?;
    send_response(
        stream,
        status,
        "application/json; charset=utf-8",
        &body,
        true,
    )
}

fn validate_fixture_name(value: &str) -> Result<(), String> {
    let path = Path::new(value);
    if value.is_empty()
        || !value.ends_with(".rlog")
        || path.file_name().and_then(|name| name.to_str()) != Some(value)
    {
        return Err("fixture must be one .rlog file name".into());
    }
    Ok(())
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
        let game_asset_root = install_root.join("assets").join(&game.manifest.game_id);
        let plugin_assets = game_asset_root.join("plugins").join(&game.folder_name);
        let shared_assets = game_asset_root.join("shared");
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
    plugins.push(combat_plugin_view(install_root));
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
    let fixtures = discover_fixtures(install_root, &mut issues);

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
            plugin_assets: install_root
                .join("assets/rlogs/plugins")
                .display()
                .to_string(),
            shared_assets: install_root
                .join("assets/rlogs/shared")
                .display()
                .to_string(),
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
        fixtures,
        issues,
    }
}

fn discover_fixtures(install_root: &Path, issues: &mut Vec<IssueView>) -> Vec<FixtureView> {
    let root = install_root.join("tests/fixtures/replay");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) => {
            issues.push(IssueView {
                scope: "replay fixtures".into(),
                detail: format!("could not read {}: {error}", root.display()),
            });
            return Vec::new();
        }
    };
    let mut fixtures = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("rlog")
            {
                return None;
            }
            let file_name = path.file_name()?.to_str()?.to_owned();
            let display_name = path.file_stem()?.to_str()?.replace(['-', '_'], " ");
            let bytes = entry.metadata().ok()?.len();
            Some(FixtureView {
                file_name,
                display_name,
                bytes,
            })
        })
        .collect::<Vec<_>>();
    fixtures.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    fixtures
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
            .join(&game.manifest.game_id)
            .join("plugins")
            .join(&game.folder_name)
            .display()
            .to_string(),
        shared_asset_namespace: install_root
            .join("assets")
            .join(&game.manifest.game_id)
            .join("shared")
            .display()
            .to_string(),
    }
}

fn combat_plugin_view(install_root: &Path) -> PluginView {
    let descriptor = CombatTimelinePlugin::new().descriptor();
    let package_path = install_root.join("plugins/builtin/combat-meter");
    PluginView {
        id: descriptor.id,
        name: descriptor.name,
        version: descriptor.version,
        folder_name: "combat-meter".into(),
        source: "built-in".into(),
        runtime: "bundled_native_replay".into(),
        api_version: PLUGIN_API_VERSION,
        compatible: true,
        capabilities: descriptor.capabilities.iter().map(enum_name).collect(),
        subscriptions: descriptor.subscriptions.iter().map(enum_name).collect(),
        export_count: 0,
        import_count: 0,
        hook_count: 0,
        package_path: package_path.display().to_string(),
        asset_namespace: install_root
            .join("assets/rlogs/plugins/combat-meter")
            .display()
            .to_string(),
        shared_asset_namespace: install_root
            .join("assets/rlogs/shared/combat-meter")
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
        assert!(
            state.fixtures.iter().any(|fixture| {
                fixture.file_name == "reference-combat.rlog" && fixture.bytes > 0
            })
        );
        assert!(state.plugins.iter().any(|plugin| {
            plugin.id == "app.rlogs.combat-meter" && plugin.runtime == "bundled_native_replay"
        }));
    }

    #[test]
    fn reference_fixture_runs_through_the_real_http_replay_path() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let body = br#"{"fixture":"reference-combat.rlog"}"#;
        let response = run_replay(&root, body).unwrap();
        assert_eq!(response.report.rlog.event_count, 13);
        assert_eq!(response.report.metrics.events_delivered, 13);
        assert_eq!(response.report.outputs.len(), 1);
    }

    #[test]
    fn replay_request_cannot_escape_the_fixture_folder() {
        assert!(validate_fixture_name("../private.rlog").is_err());
        assert!(validate_fixture_name("nested/private.rlog").is_err());
        assert!(validate_fixture_name("capture.pcapng").is_err());
    }

    #[test]
    fn current_module_optimizer_catalog_is_available_to_the_browser() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let catalog = load_optimizer_catalog(&root).unwrap();
        assert_eq!(catalog.attributes.len(), 21);
        assert_eq!(catalog.combination_sizes, [4, 5]);
        assert!(catalog.scoring_revision.contains("resonance-logs-cn-0.2.0"));
    }

    #[test]
    fn module_optimizer_http_contract_preserves_string_instance_ids() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let body = serde_json::json!({
            "modules": [
                module_fixture("9007199254740993", 4),
                module_fixture("9007199254740995", 5),
                module_fixture("9007199254740997", 6),
                module_fixture("9007199254740999", 7)
            ],
            "combination_size": 4,
            "search_mode": "exact"
        });
        let response = run_module_optimizer(&root, &serde_json::to_vec(&body).unwrap()).unwrap();
        assert_eq!(
            response.solutions[0].instance_ids,
            [
                "9007199254740993",
                "9007199254740995",
                "9007199254740997",
                "9007199254740999"
            ]
        );
        assert!(response.search.exact);
    }

    fn module_fixture(instance_id: &str, value: i32) -> serde_json::Value {
        serde_json::json!({
            "instance_id": instance_id,
            "config_id": 5500101,
            "quality": 5,
            "parts": [
                { "part_id": 1110, "initial_link_points": value },
                { "part_id": 1111, "initial_link_points": value }
            ]
        })
    }
}
