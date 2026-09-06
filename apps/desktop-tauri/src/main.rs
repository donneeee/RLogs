#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND},
    System::{
        Diagnostics::Debug::{
            GetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX, SetErrorMode,
        },
        Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
    },
    UI::{
        Shell::ShellExecuteW,
        WindowsAndMessaging::{
            GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, SW_HIDE,
            SW_SHOWNOACTIVATE, SW_SHOWNORMAL, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, ShowWindow,
            WS_EX_NOACTIVATE,
        },
    },
};

#[cfg(all(test, windows))]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, IsWindowVisible, WS_POPUP,
};

use rlogs_desktop_host::{
    COMBAT_OVERLAY_TOGGLE_ACTION_ID, EmbeddedLocalHost, HotkeyAssignmentRequest,
    HotkeyAssignmentResult, LiveCombatActivityObserver, start_embedded_local_host_with_version,
};
use serde::Serialize;
use tauri::{
    Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    webview::Color,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

fn main() {
    #[cfg(windows)]
    suppress_system_loader_dialogs();
    if let Err(error) = run() {
        let message = format!("rLogs application failed: {error}");
        eprintln!("{message}");
        #[cfg(windows)]
        show_startup_error(&message);
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn suppress_system_loader_dialogs() {
    // Capture DLL failures belong in rLogs diagnostics. Windows must never
    // replace the application with an unowned Entry Point Not Found dialog.
    unsafe {
        SetErrorMode(GetErrorMode() | SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX);
    }
}

#[cfg(windows)]
fn show_startup_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

    let title = "rLogs startup error\0".encode_utf16().collect::<Vec<_>>();
    let message = format!("{message}\0").encode_utf16().collect::<Vec<_>>();
    // SAFETY: Both strings are explicitly NUL-terminated and remain alive for
    // the duration of the synchronous Windows dialog call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[tauri::command]
fn quit_rlogs(app: tauri::AppHandle) {
    // Let Tauri begin exiting before managed state performs the potentially
    // slower capture finalization. Waiting here blocks the invoke thread and
    // makes the custom Close button appear to do nothing.
    app.exit(0);
}

#[tauri::command]
fn open_release_notes(app: tauri::AppHandle) -> Result<(), String> {
    let url = format!(
        "https://github.com/donneeee/RLogs/releases/tag/v{}",
        app.package_info().version
    );
    #[cfg(windows)]
    {
        let operation = "open\0".encode_utf16().collect::<Vec<_>>();
        let target = format!("{url}\0").encode_utf16().collect::<Vec<_>>();
        // SAFETY: Both strings are NUL-terminated and remain alive for the
        // synchronous ShellExecuteW call. No user-controlled command is run.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if result as isize <= 32 {
            return Err(format!(
                "Windows could not open the release notes ({result:p})"
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(format!(
            "Opening release notes is unsupported on this platform: {url}"
        ))
    }
}

#[derive(Default)]
struct CombatOverlayWindowState {
    ready: AtomicBool,
    requested: AtomicBool,
    automatically_hidden: AtomicBool,
    last_heartbeat_unix_millis: AtomicU64,
    last_live_update_unix_millis: AtomicU64,
    last_reload_unix_millis: AtomicU64,
    automatic_recovery_count: AtomicU64,
    consecutive_runtime_failures: AtomicU64,
}

#[derive(Default)]
struct OverlayFocusWindowState {
    hidden: AtomicBool,
    restore_labels: Mutex<BTreeSet<String>>,
}

// The combat overlay is a non-activating HUD. Pointer events still reach the
// WebView, but Windows must keep game input on the current foreground window.
const COMBAT_OVERLAY_FOCUSABLE: bool = false;
const _: () = assert!(!COMBAT_OVERLAY_FOCUSABLE);

impl OverlayFocusWindowState {
    fn allows_visibility(&self) -> bool {
        !self.hidden.load(Ordering::Acquire)
    }
}

impl CombatOverlayWindowState {
    fn from_saved_settings(enabled: bool, auto_hide_outside_combat: bool) -> Self {
        let now = unix_millis();
        Self {
            ready: AtomicBool::new(false),
            requested: AtomicBool::new(enabled),
            automatically_hidden: AtomicBool::new(!enabled || auto_hide_outside_combat),
            last_heartbeat_unix_millis: AtomicU64::new(now),
            last_live_update_unix_millis: AtomicU64::new(now),
            last_reload_unix_millis: AtomicU64::new(0),
            automatic_recovery_count: AtomicU64::new(0),
            consecutive_runtime_failures: AtomicU64::new(0),
        }
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[derive(Default)]
struct HotkeyRuntimeState {
    actions_by_shortcut_id: Mutex<BTreeMap<u32, String>>,
    registered_bindings: Mutex<BTreeMap<String, String>>,
}

#[tauri::command]
fn show_combat_overlay(
    app: tauri::AppHandle,
    state: tauri::State<'_, CombatOverlayWindowState>,
    focus_state: tauri::State<'_, OverlayFocusWindowState>,
) -> Result<(), String> {
    state.requested.store(true, Ordering::Release);
    state.automatically_hidden.store(false, Ordering::Release);
    if state.ready.load(Ordering::Acquire) && focus_state.allows_visibility() {
        let window = app
            .get_webview_window("combat-overlay")
            .ok_or_else(|| "Combat Overlay window is unavailable; restart rLogs".to_owned())?;
        show_combat_overlay_without_activation(&window)?;
        window
            .emit("combat-overlay-show-requested", ())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn set_combat_overlay_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, CombatOverlayWindowState>,
    focus_state: tauri::State<'_, OverlayFocusWindowState>,
    enabled: bool,
    automatically_hidden: bool,
) -> Result<(), String> {
    state.requested.store(enabled, Ordering::Release);
    state
        .automatically_hidden
        .store(automatically_hidden, Ordering::Release);
    let window = app
        .get_webview_window("combat-overlay")
        .ok_or_else(|| "Combat Overlay window is unavailable; restart rLogs".to_owned())?;
    if !enabled {
        return hide_combat_overlay_window(&window);
    }
    if automatically_hidden || !focus_state.allows_visibility() {
        return hide_combat_overlay_window(&window);
    }
    if state.ready.load(Ordering::Acquire) {
        show_combat_overlay_without_activation(&window)?;
        window
            .emit("combat-overlay-show-requested", ())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn hide_combat_overlay(
    app: tauri::AppHandle,
    state: tauri::State<'_, CombatOverlayWindowState>,
) -> Result<(), String> {
    // The live overlay is preloaded and reused, so hiding the WebView window
    // directly is not enough: the host would still consider it requested and
    // could reveal it again during a pending ready/show transition.
    state.requested.store(false, Ordering::Release);
    let window = app
        .get_webview_window("combat-overlay")
        .ok_or_else(|| "Combat Overlay window is unavailable; restart rLogs".to_owned())?;
    hide_combat_overlay_window(&window)
}

#[tauri::command]
fn show_combat_overlay_if_requested(
    app: tauri::AppHandle,
    state: tauri::State<'_, CombatOverlayWindowState>,
    focus_state: tauri::State<'_, OverlayFocusWindowState>,
) -> Result<(), String> {
    if !combat_overlay_should_be_visible(
        state.requested.load(Ordering::Acquire),
        state.ready.load(Ordering::Acquire),
        state.automatically_hidden.load(Ordering::Acquire),
        !focus_state.allows_visibility(),
    ) {
        return Ok(());
    }
    let window = app
        .get_webview_window("combat-overlay")
        .ok_or_else(|| "Combat Overlay window is unavailable; restart rLogs".to_owned())?;
    show_combat_overlay_without_activation(&window)
}

#[tauri::command]
fn set_combat_overlay_automatically_hidden(
    app: tauri::AppHandle,
    state: tauri::State<'_, CombatOverlayWindowState>,
    focus_state: tauri::State<'_, OverlayFocusWindowState>,
    hidden: bool,
) -> Result<(), String> {
    state.automatically_hidden.store(hidden, Ordering::Release);
    let window = app
        .get_webview_window("combat-overlay")
        .ok_or_else(|| "Combat Overlay window is unavailable; restart rLogs".to_owned())?;
    if hidden {
        return hide_combat_overlay_window(&window);
    }
    if combat_overlay_should_be_visible(
        state.requested.load(Ordering::Acquire),
        state.ready.load(Ordering::Acquire),
        false,
        !focus_state.allows_visibility(),
    ) {
        show_combat_overlay_without_activation(&window)?;
    }
    Ok(())
}

fn combat_overlay_should_be_visible(
    requested: bool,
    ready: bool,
    automatically_hidden: bool,
    hidden_by_focus_policy: bool,
) -> bool {
    requested && ready && !automatically_hidden && !hidden_by_focus_policy
}

fn combat_overlay_hostile_activity_started(
    previous_hostile_micros: Option<u64>,
    current_hostile_micros: Option<u64>,
) -> bool {
    current_hostile_micros.is_some() && current_hostile_micros != previous_hostile_micros
}

fn combat_overlay_damage_started(previous_count: u64, current_count: u64) -> bool {
    current_count > previous_count
}

fn toggle_combat_overlay(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<CombatOverlayWindowState>();
    let focus_state = app.state::<OverlayFocusWindowState>();
    let window = app
        .get_webview_window("combat-overlay")
        .ok_or_else(|| "Combat Overlay window is unavailable; restart rLogs".to_owned())?;
    let visibly_requested = state.requested.load(Ordering::Acquire)
        && !state.automatically_hidden.load(Ordering::Acquire)
        && window.is_visible().map_err(|error| error.to_string())?;
    if visibly_requested {
        state.requested.store(false, Ordering::Release);
        hide_combat_overlay_window(&window)?;
    } else {
        state.requested.store(true, Ordering::Release);
        state.automatically_hidden.store(false, Ordering::Release);
        if state.ready.load(Ordering::Acquire) && focus_state.allows_visibility() {
            show_combat_overlay_without_activation(&window)?;
            window
                .emit("combat-overlay-show-requested", ())
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn install_global_hotkeys(
    app: &tauri::AppHandle,
    bindings: &BTreeMap<String, String>,
) -> Result<(), String> {
    let parsed = bindings
        .iter()
        .map(|(action_id, binding)| {
            binding
                .parse::<Shortcut>()
                .map(|shortcut| (action_id.clone(), binding.clone(), shortcut))
                .map_err(|error| format!("{binding} is not a supported global shortcut: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let runtime = app.state::<HotkeyRuntimeState>();
    let previous = runtime
        .registered_bindings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();

    app.global_shortcut()
        .unregister_all()
        .map_err(|error| format!("could not replace the registered hotkeys: {error}"))?;
    for (_, binding, shortcut) in &parsed {
        if let Err(error) = app.global_shortcut().register(*shortcut) {
            let _ = app.global_shortcut().unregister_all();
            for old_binding in previous.values() {
                let _ = app.global_shortcut().register(old_binding.as_str());
            }
            return Err(format!(
                "could not register {binding}; it may already be used by another application: {error}"
            ));
        }
    }

    *runtime
        .actions_by_shortcut_id
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = parsed
        .iter()
        .map(|(action_id, _, shortcut)| (shortcut.id(), action_id.clone()))
        .collect();
    *runtime
        .registered_bindings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = bindings.clone();
    Ok(())
}

#[tauri::command]
fn assign_hotkey(
    app: tauri::AppHandle,
    host: tauri::State<'_, EmbeddedLocalHost>,
    assignment: HotkeyAssignmentRequest,
) -> Result<HotkeyAssignmentResult, String> {
    let previous = host.hotkey_settings().bindings;
    let result = host.assign_hotkey(assignment)?;
    if let Err(error) = install_global_hotkeys(&app, &result.settings.bindings) {
        host.restore_hotkey_bindings(previous.clone())?;
        install_global_hotkeys(&app, &previous)?;
        return Err(error);
    }
    Ok(result)
}

fn build_combat_overlay_window(
    app: &mut tauri::App,
    host: &rlogs_desktop_host::EmbeddedLocalHost,
) -> tauri::Result<()> {
    let url = format!("http://{}/?surface=combat-overlay", host.address())
        .parse()
        .map_err(tauri::Error::InvalidUrl)?;
    WebviewWindowBuilder::new(app, "combat-overlay", WebviewUrl::External(url))
        .title("rLogs Combat Overlay")
        .decorations(false)
        // A visible transparent Windows host otherwise retains a faint native
        // frame while CSS hides the overlay between combat windows.
        .shadow(false)
        .transparent(true)
        .background_color(Color(11, 21, 34, 0))
        // WebView2 creates its native surface before it can paint the page.
        // Keep that surface hidden until the runtime confirms that its first
        // frame exists, otherwise Windows exposes a large white rectangle.
        .visible(false)
        // The live meter is a heads-up display. On Windows this adds
        // WS_EX_NOACTIVATE, so showing it or reloading WebView2 cannot take
        // keyboard/controller focus away from the game.
        .focusable(COMBAT_OVERLAY_FOCUSABLE)
        .focused(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .inner_size(460.0, 520.0)
        .min_inner_size(160.0, 80.0)
        .resizable(true)
        .build()
        .map(|_| ())?;
    Ok(())
}

#[tauri::command]
fn show_event_inspector(
    app: tauri::AppHandle,
    host: tauri::State<'_, EmbeddedLocalHost>,
) -> Result<(), String> {
    if !cfg!(debug_assertions) && !host.core_settings().developer_mode {
        return Err("Enable Developer mode in Settings before opening Event Inspector.".into());
    }
    if let Some(window) = app.get_webview_window("event-inspector") {
        if window.is_minimized().map_err(|error| error.to_string())? {
            window.unminimize().map_err(|error| error.to_string())?;
        }
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }
    let url = format!("http://{}/?surface=event-inspector", host.address())
        .parse::<tauri::Url>()
        .map_err(|error| error.to_string())?;
    WebviewWindowBuilder::new(&app, "event-inspector", WebviewUrl::External(url))
        .title("rLogs Event Inspector")
        .decorations(true)
        .inner_size(1280.0, 820.0)
        .min_inner_size(780.0, 560.0)
        .resizable(true)
        .skip_taskbar(false)
        .build()
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn close_event_inspector(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("event-inspector") {
        window.close().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn combat_overlay_ready(
    app: tauri::AppHandle,
    state: tauri::State<'_, CombatOverlayWindowState>,
    focus_state: tauri::State<'_, OverlayFocusWindowState>,
) -> Result<(), String> {
    state.ready.store(true, Ordering::Release);
    let requested = state.requested.load(Ordering::Acquire);
    let automatically_hidden = state.automatically_hidden.load(Ordering::Acquire);
    if requested && automatically_hidden {
        let window = app
            .get_webview_window("combat-overlay")
            .ok_or_else(|| "Combat Overlay window no longer exists".to_owned())?;
        hide_combat_overlay_window(&window)?;
        return Ok(());
    }
    if combat_overlay_should_be_visible(
        requested,
        true,
        automatically_hidden,
        !focus_state.allows_visibility(),
    ) {
        let window = app
            .get_webview_window("combat-overlay")
            .ok_or_else(|| "Combat Overlay window no longer exists".to_owned())?;
        show_combat_overlay_without_activation(&window)?;
    }
    Ok(())
}

fn show_combat_overlay_without_activation(window: &tauri::WebviewWindow) -> Result<(), String> {
    // Re-assert the invariant at runtime as well as in the builder. A WebView2
    // renderer reload must never turn the combat HUD into an activatable
    // window before its ready callback reveals it again.
    window
        .set_focusable(COMBAT_OVERLAY_FOCUSABLE)
        .map_err(|error| error.to_string())?;
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|error| error.to_string())?.0;
        show_native_combat_overlay_without_activation(hwnd)
    }
    #[cfg(not(windows))]
    {
        window.show().map_err(|error| error.to_string())
    }
}

#[cfg(windows)]
fn show_native_combat_overlay_without_activation(hwnd: HWND) -> Result<(), String> {
    // Tauri's generic `show` operation ultimately uses SW_SHOW after the first
    // reveal. That operation is allowed to activate a window even when the
    // builder originally requested no focus. The overlay is frequently
    // hidden/revealed and may also be reloaded by the renderer watchdog, so
    // enforce the native contract on every reveal.
    unsafe {
        let extended_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            extended_style | WS_EX_NOACTIVATE as isize,
        );
        if SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        ) == 0
        {
            return Err(format!(
                "Windows could not apply the non-activating overlay style: {}",
                std::io::Error::last_os_error()
            ));
        }
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    Ok(())
}

fn hide_combat_overlay_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|error| error.to_string())?.0;
        // Keep native visibility paired with the native no-activate reveal.
        // Mixing this with Tauri's cached visibility flag can make a later
        // generic show use SW_SHOW and take focus from the game.
        unsafe {
            ShowWindow(hwnd, SW_HIDE);
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        window.hide().map_err(|error| error.to_string())
    }
}

#[tauri::command]
fn combat_overlay_heartbeat(
    state: tauri::State<'_, CombatOverlayWindowState>,
    consecutive_failures: u64,
    last_successful_update_unix_millis: u64,
) {
    let now = unix_millis();
    state
        .last_heartbeat_unix_millis
        .store(now, Ordering::Release);
    state.last_live_update_unix_millis.store(
        last_successful_update_unix_millis.min(now),
        Ordering::Release,
    );
    state
        .consecutive_runtime_failures
        .store(consecutive_failures, Ordering::Release);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CombatOverlayHealth {
    status: &'static str,
    requested: bool,
    ready: bool,
    visible: bool,
    automatically_hidden: bool,
    hidden_by_focus_policy: bool,
    last_heartbeat_unix_millis: u64,
    heartbeat_age_millis: u64,
    last_live_update_unix_millis: u64,
    live_update_age_millis: u64,
    consecutive_runtime_failures: u64,
    automatic_recovery_count: u64,
    last_recovery_unix_millis: Option<u64>,
}

fn combat_overlay_health_status(
    requested: bool,
    ready: bool,
    visible: bool,
    automatically_hidden: bool,
    hidden_by_focus_policy: bool,
    liveness_ages_millis: (u64, u64),
    consecutive_runtime_failures: u64,
) -> &'static str {
    let (heartbeat_age_millis, live_update_age_millis) = liveness_ages_millis;
    if !requested {
        "disabled"
    } else if automatically_hidden {
        "auto_hidden"
    } else if hidden_by_focus_policy {
        "focus_hidden"
    } else if !ready {
        "starting"
    } else if consecutive_runtime_failures > 0 {
        "reconnecting"
    } else if heartbeat_age_millis >= 15_000 {
        "stalled"
    } else if live_update_age_millis >= 15_000 {
        "feed_stalled"
    } else if visible {
        "healthy"
    } else {
        "window_hidden"
    }
}

#[tauri::command]
fn combat_overlay_health(
    app: tauri::AppHandle,
    state: tauri::State<'_, CombatOverlayWindowState>,
    focus_state: tauri::State<'_, OverlayFocusWindowState>,
) -> CombatOverlayHealth {
    let now = unix_millis();
    let requested = state.requested.load(Ordering::Acquire);
    let ready = state.ready.load(Ordering::Acquire);
    let automatically_hidden = state.automatically_hidden.load(Ordering::Acquire);
    let hidden_by_focus_policy = !focus_state.allows_visibility();
    let visible = app
        .get_webview_window("combat-overlay")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    let last_heartbeat_unix_millis = state.last_heartbeat_unix_millis.load(Ordering::Acquire);
    let heartbeat_age_millis = now.saturating_sub(last_heartbeat_unix_millis);
    let last_live_update_unix_millis = state.last_live_update_unix_millis.load(Ordering::Acquire);
    let live_update_age_millis = now.saturating_sub(last_live_update_unix_millis);
    let consecutive_runtime_failures = state.consecutive_runtime_failures.load(Ordering::Acquire);
    let automatic_recovery_count = state.automatic_recovery_count.load(Ordering::Acquire);
    let last_recovery = state.last_reload_unix_millis.load(Ordering::Acquire);
    let status = combat_overlay_health_status(
        requested,
        ready,
        visible,
        automatically_hidden,
        hidden_by_focus_policy,
        (heartbeat_age_millis, live_update_age_millis),
        consecutive_runtime_failures,
    );
    CombatOverlayHealth {
        status,
        requested,
        ready,
        visible,
        automatically_hidden,
        hidden_by_focus_policy,
        last_heartbeat_unix_millis,
        heartbeat_age_millis,
        last_live_update_unix_millis,
        live_update_age_millis,
        consecutive_runtime_failures,
        automatic_recovery_count,
        last_recovery_unix_millis: (last_recovery > 0).then_some(last_recovery),
    }
}

fn combat_overlay_renderer_is_stale(now: u64, last_heartbeat: u64, last_reload: u64) -> bool {
    now.saturating_sub(last_heartbeat) >= 15_000
        && (last_reload == 0 || now.saturating_sub(last_reload) >= 30_000)
}

fn monitor_combat_overlay_renderer(app: tauri::AppHandle) -> std::io::Result<()> {
    thread::Builder::new()
        .name("rlogs-overlay-renderer".into())
        .spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(5));
                let state = app.state::<CombatOverlayWindowState>();
                if !state.requested.load(Ordering::Acquire)
                    || state.automatically_hidden.load(Ordering::Acquire)
                    || !state.ready.load(Ordering::Acquire)
                {
                    continue;
                }
                let Some(window) = app.get_webview_window("combat-overlay") else {
                    continue;
                };
                if !window.is_visible().unwrap_or(false) {
                    continue;
                }
                let now = unix_millis();
                let last_heartbeat = state.last_heartbeat_unix_millis.load(Ordering::Acquire);
                let last_reload = state.last_reload_unix_millis.load(Ordering::Acquire);
                if !combat_overlay_renderer_is_stale(now, last_heartbeat, last_reload) {
                    continue;
                }
                state.last_reload_unix_millis.store(now, Ordering::Release);
                state.ready.store(false, Ordering::Release);
                // A WebView2 navigation can transiently activate its child
                // surface. Reload it while the native overlay is hidden, then
                // let `combat_overlay_ready` reveal it with SW_SHOWNOACTIVATE.
                let _ = hide_combat_overlay_window(&window);
                if window.reload().is_err() {
                    // A failed reload must not permanently suppress the otherwise
                    // live window; the next watchdog interval may try again.
                    state.ready.store(true, Ordering::Release);
                    let _ = show_combat_overlay_without_activation(&window);
                } else {
                    state
                        .automatic_recovery_count
                        .fetch_add(1, Ordering::AcqRel);
                }
            }
        })
        .map(|_| ())
}

fn monitor_combat_overlay_activity(
    app: tauri::AppHandle,
    observer: LiveCombatActivityObserver,
) -> std::io::Result<()> {
    thread::Builder::new()
        .name("rlogs-overlay-wake".into())
        .spawn(move || {
            // Treat the already-published snapshot as a baseline. A stale
            // active snapshot must not reveal the overlay at application
            // startup; only a newly observed hostile event should wake it.
            let initial = observer.current();
            let mut revision = initial.revision;
            let mut last_hostile_micros = initial.last_hostile_micros;
            let mut damage_event_count = initial.damage_event_count;
            loop {
                let update = observer.wait_after(revision, Duration::from_secs(1));
                let feed_changed = update.revision > revision;
                revision = revision.max(update.revision);
                let damage_started = feed_changed
                    && combat_overlay_damage_started(damage_event_count, update.damage_event_count);
                let hostile_activity_started = feed_changed
                    && (damage_started
                        || (update.combat_active
                            && combat_overlay_hostile_activity_started(
                                last_hostile_micros,
                                update.last_hostile_micros,
                            )));
                last_hostile_micros = update.last_hostile_micros;
                damage_event_count = update.damage_event_count;
                if !hostile_activity_started {
                    continue;
                }
                let Some(window) = app.get_webview_window("combat-overlay") else {
                    return;
                };
                let state = app.state::<CombatOverlayWindowState>();
                let focus_state = app.state::<OverlayFocusWindowState>();
                if !state.requested.load(Ordering::Acquire) || !state.ready.load(Ordering::Acquire)
                {
                    continue;
                }
                // Focus hiding is only a native-window gate. Preserve the
                // overlay's own combat visibility state so combat that starts
                // while another app is foreground can be shown immediately
                // when the game or rLogs regains focus.
                state.automatically_hidden.store(false, Ordering::Release);
                if !focus_state.allows_visibility() {
                    continue;
                }
                // A direct decoded-damage signal wakes the physically hidden
                // native window. Keeping it visibly parked leaves a stale
                // WebView2 compositor rectangle on Windows, so automatic
                // hiding must use the real native visibility state.
                // Do not steal focus when combat begins. Revealing the
                // always-on-top overlay is sufficient and keeps game input in
                // the game.
                let _ = show_combat_overlay_without_activation(&window);
                // The WebView root is hidden separately from its native
                // window. Reconcile both states after waking so the native
                // surface cannot be shown with transparent content.
                let _ = window.emit("combat-overlay-show-requested", ());
            }
        })
        .map(|_| ())
}

fn is_overlay_window_label(label: &str) -> bool {
    label.ends_with("-overlay")
}

fn set_overlay_windows_hidden_by_focus(app: &tauri::AppHandle, hidden: bool) {
    let focus_state = app.state::<OverlayFocusWindowState>();
    if hidden {
        let was_hidden = focus_state.hidden.swap(true, Ordering::AcqRel);
        let mut restore_labels = focus_state
            .restore_labels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !was_hidden {
            restore_labels.clear();
        }
        for (label, window) in app.webview_windows() {
            if !is_overlay_window_label(&label) {
                continue;
            }
            if window.is_visible().unwrap_or(false) {
                restore_labels.insert(label);
                if window.label() == "combat-overlay" {
                    let _ = hide_combat_overlay_window(&window);
                } else {
                    let _ = window.hide();
                }
            }
        }
        return;
    }

    if !focus_state.hidden.swap(false, Ordering::AcqRel) {
        return;
    }
    let restore_labels = std::mem::take(
        &mut *focus_state
            .restore_labels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    for label in restore_labels {
        if label == "combat-overlay" {
            continue;
        }
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.show();
        }
    }
    let combat = app.state::<CombatOverlayWindowState>();
    if combat_overlay_should_be_visible(
        combat.requested.load(Ordering::Acquire),
        combat.ready.load(Ordering::Acquire),
        combat.automatically_hidden.load(Ordering::Acquire),
        false,
    ) && let Some(window) = app.get_webview_window("combat-overlay")
    {
        let _ = show_combat_overlay_without_activation(&window);
        let _ = window.emit("combat-overlay-show-requested", ());
    }
}

#[cfg(windows)]
fn foreground_process_name() -> Option<String> {
    // SAFETY: Windows owns the returned foreground HWND. The process handle is
    // opened read-only, checked, and closed exactly once.
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return None;
    }
    let mut process_id = 0_u32;
    unsafe { GetWindowThreadProcessId(window, &mut process_id) };
    if process_id == 0 {
        return None;
    }
    if process_id == std::process::id() {
        return Some(String::new());
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return None;
    }
    let result = (|| {
        let mut path = vec![0_u16; 32_768];
        let mut length = u32::try_from(path.len()).ok()?;
        if unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) } == 0 {
            return None;
        }
        path.truncate(length as usize);
        std::path::PathBuf::from(String::from_utf16_lossy(&path))
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    })();
    unsafe { CloseHandle(process) };
    result
}

#[cfg(windows)]
fn game_or_rlogs_is_foreground(game_process_names: &[String]) -> bool {
    foreground_process_name().is_some_and(|name| {
        name.is_empty()
            || game_process_names
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(&name))
    })
}

#[cfg(not(windows))]
fn game_or_rlogs_is_foreground(_: &[String]) -> bool {
    // Platform adapters may opt into foreground detection without changing the
    // shared overlay policy. Until then, fail open rather than hiding Linux
    // overlays based on an unsupported compositor query.
    true
}

#[derive(Debug, Default)]
struct OverlayFocusPolicyDebounce {
    applied_hidden: bool,
    candidate_hidden: bool,
    consecutive_samples: u8,
}

impl OverlayFocusPolicyDebounce {
    fn observe(&mut self, desired_hidden: bool) -> Option<bool> {
        if desired_hidden == self.applied_hidden {
            self.candidate_hidden = desired_hidden;
            self.consecutive_samples = 0;
            return None;
        }
        if desired_hidden != self.candidate_hidden {
            self.candidate_hidden = desired_hidden;
            self.consecutive_samples = 1;
        } else {
            self.consecutive_samples = self.consecutive_samples.saturating_add(1);
        }
        // Windows can briefly report a shell/IME/helper window between game
        // frames. Require a stable foreground transition before physically
        // hiding or revealing every overlay window.
        let required_samples = if desired_hidden { 4 } else { 2 };
        if self.consecutive_samples < required_samples {
            return None;
        }
        self.applied_hidden = desired_hidden;
        self.consecutive_samples = 0;
        Some(desired_hidden)
    }
}

fn monitor_overlay_focus_policy(
    app: tauri::AppHandle,
    game_process_names: Vec<String>,
) -> std::io::Result<()> {
    thread::Builder::new()
        .name("rlogs-overlay-focus".into())
        .spawn(move || {
            let mut debounce = OverlayFocusPolicyDebounce::default();
            loop {
                let enabled = app
                    .state::<EmbeddedLocalHost>()
                    .core_settings()
                    .hide_overlays_when_unfocused;
                let hidden = enabled && !game_or_rlogs_is_foreground(&game_process_names);
                if let Some(stable_hidden) = debounce.observe(hidden) {
                    set_overlay_windows_hidden_by_focus(&app, stable_hidden);
                }
                thread::sleep(Duration::from_millis(150));
            }
        })
        .map(|_| ())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    let action_id = app
                        .state::<HotkeyRuntimeState>()
                        .actions_by_shortcut_id
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .get(&shortcut.id())
                        .cloned();
                    if action_id.as_deref() == Some(COMBAT_OVERLAY_TOGGLE_ACTION_ID) {
                        let _ = toggle_combat_overlay(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let install_root = application_install_root(app)?;
            let host = start_embedded_local_host_with_version(
                &install_root,
                app.package_info().version.to_string(),
            )?;
            let url = format!("http://{}", host.address()).parse()?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("rLogs")
                .decorations(false)
                .inner_size(1280.0, 800.0)
                .min_inner_size(960.0, 620.0)
                .resizable(true)
                .build()?;
            let (overlay_enabled, overlay_auto_hide) = host.combat_overlay_startup_state();
            app.manage(CombatOverlayWindowState::from_saved_settings(
                overlay_enabled,
                overlay_auto_hide,
            ));
            app.manage(OverlayFocusWindowState::default());
            app.manage(HotkeyRuntimeState::default());
            build_combat_overlay_window(app, &host)?;
            let game_process_names = host.foreground_game_process_names();
            let combat_observer = host.live_combat_activity_observer();
            app.manage(host);
            monitor_combat_overlay_activity(app.handle().clone(), combat_observer)?;
            monitor_overlay_focus_policy(app.handle().clone(), game_process_names)?;
            monitor_combat_overlay_renderer(app.handle().clone())?;
            install_global_hotkeys(
                app.handle(),
                &app.state::<EmbeddedLocalHost>().hotkey_settings().bindings,
            )?;
            install_tray(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            quit_rlogs,
            open_release_notes,
            show_event_inspector,
            close_event_inspector,
            show_combat_overlay,
            set_combat_overlay_enabled,
            hide_combat_overlay,
            show_combat_overlay_if_requested,
            set_combat_overlay_automatically_hidden,
            combat_overlay_ready,
            combat_overlay_heartbeat,
            combat_overlay_health,
            assign_hotkey
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        OverlayFocusPolicyDebounce, combat_overlay_damage_started, combat_overlay_health_status,
        combat_overlay_hostile_activity_started, combat_overlay_renderer_is_stale,
        combat_overlay_should_be_visible, is_overlay_window_label,
    };

    #[test]
    fn overlay_requires_request_readiness_and_nonautomatic_visibility() {
        assert!(combat_overlay_should_be_visible(true, true, false, false));
        assert!(!combat_overlay_should_be_visible(false, true, false, false));
        assert!(!combat_overlay_should_be_visible(true, false, false, false));
        assert!(!combat_overlay_should_be_visible(true, true, true, false));
        assert!(!combat_overlay_should_be_visible(true, true, false, true));
    }

    #[test]
    fn overlay_wakes_for_each_new_hostile_timestamp() {
        assert!(combat_overlay_hostile_activity_started(None, Some(10)));
        assert!(combat_overlay_hostile_activity_started(Some(10), Some(20)));
        assert!(!combat_overlay_hostile_activity_started(Some(10), Some(10)));
        assert!(!combat_overlay_hostile_activity_started(Some(10), None));
        assert!(!combat_overlay_hostile_activity_started(None, None));
    }

    #[test]
    fn overlay_wakes_only_when_damage_count_advances() {
        assert!(combat_overlay_damage_started(0, 1));
        assert!(combat_overlay_damage_started(20, 21));
        assert!(!combat_overlay_damage_started(20, 20));
        assert!(!combat_overlay_damage_started(20, 0));
    }

    #[test]
    fn visible_overlay_renderer_recovers_after_a_bounded_stall() {
        assert!(!combat_overlay_renderer_is_stale(14_999, 0, 0));
        assert!(combat_overlay_renderer_is_stale(15_000, 0, 0));
        assert!(!combat_overlay_renderer_is_stale(44_999, 0, 30_000));
        assert!(combat_overlay_renderer_is_stale(60_000, 0, 30_000));
        assert!(!combat_overlay_renderer_is_stale(20_000, 19_000, 0));
    }

    #[test]
    fn live_feed_silence_does_not_trigger_renderer_recovery() {
        // The feed's age is deliberately absent from the renderer predicate.
        // A healthy HUD must remain loaded while the player is out of combat.
        assert!(!combat_overlay_renderer_is_stale(86_400_000, 86_399_000, 0));
    }

    #[cfg(windows)]
    #[test]
    fn native_overlay_reveal_is_visible_but_never_foreground() {
        use super::{
            CreateWindowExW, DestroyWindow, GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW,
            IsWindowVisible, WS_EX_NOACTIVATE, WS_POPUP,
            show_native_combat_overlay_without_activation,
        };

        let class_name = "STATIC\0".encode_utf16().collect::<Vec<_>>();
        let window_name = "rLogs non-activating overlay test\0"
            .encode_utf16()
            .collect::<Vec<_>>();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                window_name.as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null(), "the native test window must be created");

        show_native_combat_overlay_without_activation(hwnd).unwrap();
        assert_ne!(unsafe { IsWindowVisible(hwnd) }, 0);
        assert_ne!(unsafe { GetForegroundWindow() }, hwnd);
        assert_ne!(
            unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } & WS_EX_NOACTIVATE as isize,
            0
        );

        unsafe {
            DestroyWindow(hwnd);
        }
    }

    #[test]
    fn overlay_health_distinguishes_intentional_hiding_from_failures() {
        assert_eq!(
            combat_overlay_health_status(false, true, false, false, false, (0, 0), 0),
            "disabled"
        );
        assert_eq!(
            combat_overlay_health_status(true, true, false, true, false, (60_000, 60_000), 0),
            "auto_hidden"
        );
        assert_eq!(
            combat_overlay_health_status(true, true, true, false, false, (0, 0), 2),
            "reconnecting"
        );
        assert_eq!(
            combat_overlay_health_status(true, true, true, false, false, (15_000, 0), 0),
            "stalled"
        );
        assert_eq!(
            combat_overlay_health_status(true, true, true, false, false, (1_000, 1_000), 0),
            "healthy"
        );
        assert_eq!(
            combat_overlay_health_status(true, true, true, false, false, (1_000, 15_000), 0),
            "feed_stalled"
        );
    }

    #[test]
    fn focus_policy_recognizes_present_and_future_overlay_windows() {
        assert!(is_overlay_window_label("combat-overlay"));
        assert!(is_overlay_window_label("cooldown-overlay"));
        assert!(is_overlay_window_label("map-overlay"));
        assert!(!is_overlay_window_label("main"));
        assert!(!is_overlay_window_label("settings"));
    }

    #[test]
    fn focus_policy_ignores_transient_foreground_misses() {
        let mut debounce = OverlayFocusPolicyDebounce::default();
        assert_eq!(debounce.observe(true), None);
        assert_eq!(debounce.observe(false), None);
        assert_eq!(debounce.observe(true), None);
        assert_eq!(debounce.observe(true), None);
        assert_eq!(debounce.observe(true), None);
        assert_eq!(debounce.observe(true), Some(true));
        assert_eq!(debounce.observe(false), None);
        assert_eq!(debounce.observe(false), Some(false));
    }
}

fn install_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show rLogs", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit rLogs", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .tooltip("rLogs")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => {
                app.exit(0);
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn application_install_root(app: &tauri::App) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = std::env::var_os("RLOGS_INSTALL_ROOT") {
        return Ok(std::fs::canonicalize(path)?);
    }
    if cfg!(debug_assertions) {
        return Ok(std::fs::canonicalize(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        )?);
    }
    Ok(std::fs::canonicalize(app.path().resource_dir()?)?)
}
