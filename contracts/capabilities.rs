//! Public capabilities shared by discovery and package compatibility checks.
pub const API_VERSION: &str = "4.8.0";
pub const CAPABILITIES: &[&str] = &[
    "servers.catalog.v1",
    "plugins.metadata.v1", "workspace.contributions.v2", "ui.dialog.v1",
    "network.tcp.v1", "network.stream.v1", "network.resources.v1",
    "servers.routes.v2", "connections.route.v2", "servers.routes.v1", "connections.route.v1", "workspace.directory.connection.v1",
    "core.state.v1", "workspace.directory.v1", "remote.pty.observe.v1", "remote.pty.start.v1",
    "servers.v1", "credentials.v1", "connections.v1", "remote.pty.v1", "pty.v1",
    "resources.v1", "storage.v1", "settings.v1", "calls.v1", "workspace.v4",
    "ui.theme.catalog.v1", "ui.theme.dopamine.v1",
    "ui.presentation.v1", "ui.theme.live.v1", "diagnostics.v1", "clipboard.v1", "plugin.invoke.v1",
    "remote.files.read.v1", "remote.exec.v1", "connections.observe.v1",
    "workspace.multiplugin.v1", "workspace.presentation.v1",
];
