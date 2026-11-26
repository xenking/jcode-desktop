//! The MCP submenu of the `+` picker: the servers jcode is configured to run,
//! read from the same files the runtime reads, with the tools each one
//! offered the last time it connected.
use jcode_base::mcp::{McpConfig, McpSchemaCache, McpServerConfig};
use std::path::Path;

/// One configured server as the picker lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Server {
    pub(crate) name: String,
    pub(crate) summary: String,
}

/// Servers from `~/.jcode/mcp.json`, the Claude Code files and the project's
/// own files, plus the tool names the runtime cached. Blocking file reads:
/// call off the UI thread.
pub(crate) fn load(project: Option<&Path>) -> Vec<Server> {
    servers(&McpConfig::load_for_dir(project), &McpSchemaCache::load())
}

fn servers(config: &McpConfig, cache: &McpSchemaCache) -> Vec<Server> {
    let mut servers: Vec<_> = config
        .servers
        .iter()
        .map(|(name, server)| Server {
            name: name.clone(),
            summary: summary(name, server, cache),
        })
        .collect();
    servers.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    servers
}

/// "3 tools: search, fetch, read" when the runtime has seen the server's
/// tools, otherwise where the server runs. A disabled server says so.
fn summary(name: &str, server: &McpServerConfig, cache: &McpSchemaCache) -> String {
    let mut parts = Vec::new();
    if !server.is_enabled() {
        parts.push("disabled".to_owned());
    }
    match cache.tools_for(name, server) {
        Some(tools) if !tools.is_empty() => {
            let names: Vec<_> = tools.iter().map(|tool| tool.name.as_str()).collect();
            let noun = if names.len() == 1 { "tool" } else { "tools" };
            parts.push(format!("{} {noun}: {}", names.len(), names.join(", ")));
        }
        _ => parts.push(server.remote_transport().map_or_else(
            || "local server".to_owned(),
            |kind| format!("{} server", kind.label()),
        )),
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(json: &str) -> McpConfig {
        serde_json::from_str(json).expect("config parses")
    }

    #[test]
    fn servers_list_by_name_with_their_cached_tools_or_their_kind() {
        let config = config(
            r#"{"mcpServers":{
                "Zed":{"command":"zed-mcp"},
                "alpha":{"command":"alpha-mcp"},
                "remote":{"type":"http","url":"https://example.invalid/mcp"},
                "off":{"command":"off-mcp","disabled":true}
            }}"#,
        );
        let alpha = &config.servers["alpha"];
        let cache: McpSchemaCache = serde_json::from_value(serde_json::json!({
            "version": 2,
            "servers": {"alpha": {
                "fingerprint": jcode_base::mcp::fingerprint_config(alpha),
                "tools": [{"name": "search", "inputSchema": {}}, {"name": "fetch", "inputSchema": {}}]
            }}
        }))
        .expect("cache parses");
        let listed: Vec<_> = servers(&config, &cache)
            .into_iter()
            .map(|server| (server.name, server.summary))
            .collect();
        assert_eq!(
            listed,
            [
                ("alpha".into(), "2 tools: search, fetch".into()),
                ("off".into(), "disabled · local server".into()),
                ("remote".into(), "http server".into()),
                ("Zed".into(), "local server".into()),
            ]
        );
    }

    #[test]
    fn tools_cached_for_a_reconfigured_server_are_not_claimed() {
        let config = config(r#"{"servers":{"alpha":{"command":"alpha-mcp","args":["--new"]}}}"#);
        let cache: McpSchemaCache = serde_json::from_value(serde_json::json!({
            "version": 2,
            "servers": {"alpha": {"fingerprint": "an-older-config", "tools": [{"name": "search", "inputSchema": {}}]}}
        }))
        .expect("cache parses");
        assert_eq!(servers(&config, &cache)[0].summary, "local server");
    }
}
