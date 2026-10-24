fn main() {
    std::process::exit(sodeck_plugin_host::run_internal_host_if_requested().unwrap_or(2));
}
