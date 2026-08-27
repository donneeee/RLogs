fn main() {
    if let Err(error) = rlogs_desktop_host::run_browser_host_from_env() {
        eprintln!("rLogs local host failed: {error}");
        std::process::exit(1);
    }
}
