//! Registering the MCP server with the AI clients on this machine —
//! Claude Desktop (its JSON config), Claude Code and Codex (their CLIs) —
//! plus the copy-paste setup info for everything else. Detection reports
//! disk/CLI truth, never UI state: the frontend refetches after every
//! action instead of assuming success ([integrations.md](../../docs/integrations.md)).
//! Where Claude Desktop keeps its config (and how CLIs resolve) is
//! per-OS — `crate::platform::{mcp_paths, find_cli}` own that.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the MCP server binary lives plus ready-made client snippets for the
/// Settings → MCP page. In dev the workspace `target/` build is used; in a
/// bundled install the sidecar sits next to the app executable (bundling is
/// wired up in R6 release engineering).
#[derive(serde::Serialize)]
pub struct McpSetupInfo {
    pub path: String,
    pub exists: bool,
    pub claude_code_command: String,
    pub config_json: String,
    pub codex_command: String,
    pub codex_toml: String,
    pub claude_desktop_config_path: String,
}

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpClient {
    ClaudeDesktop,
    ClaudeCode,
    Codex,
}

#[derive(serde::Serialize)]
pub struct ClientStatus {
    pub installed: bool,
    pub registered: bool,
    /// The resolved CLI/config path, or why detection came up empty — the
    /// UI's status line.
    pub detail: String,
}

#[derive(serde::Serialize)]
pub struct McpClientsStatus {
    pub server_path: String,
    pub server_exists: bool,
    pub claude_desktop: ClientStatus,
    pub claude_code: ClientStatus,
    pub codex: ClientStatus,
}

// --- Paths and resolution ---

/// The server binary: bundled sidecar next to the app exe first, then the
/// workspace's release and debug builds (dev). The final fallback is the
/// sidecar path even when absent, so the UI can say what's missing.
/// Also the app's embedding worker — `search_index` spawns this same
/// binary in its `embed` mode.
pub(crate) fn server_binary() -> Result<(PathBuf, bool), String> {
    let exe = crate::platform::exe_name("embral-mcp");
    let exe_dir_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(&exe)));
    let dev_release_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join("release")
        .join(&exe);
    let dev_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join("debug")
        .join(&exe);

    let path = [exe_dir_path.clone(), Some(dev_release_path), Some(dev_path)]
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
        .or(exe_dir_path)
        .ok_or("could not resolve the embral-mcp path")?;
    let exists = path.is_file();
    Ok((path, exists))
}

/// Canonicalize without Windows `\\?\` prefixes (which confuse copied configs).
fn dunce_simplify(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = canonical.to_string_lossy().to_string();
    s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
}

const DESKTOP_CONFIG_FILE: &str = "claude_desktop_config.json";

use crate::platform::mcp_paths::{desktop_config_dir_on_disk, resolve_desktop_config_dir};

/// Claude Code's user-scope registry (`claude mcp add --scope user` writes it).
fn claude_code_config() -> Option<PathBuf> {
    dirs::home_dir().map(|d| d.join(".claude.json"))
}

fn codex_config() -> Option<PathBuf> {
    dirs::home_dir().map(|d| d.join(".codex").join("config.toml"))
}

// --- Pure config-file logic (tested) ---

/// Set `mcpServers.embral` in a Claude-style JSON config, preserving every
/// other key byte-for-byte semantically. Refuses input it can't parse —
/// never clobber a config we couldn't read.
fn upsert_mcp_server(existing: &str, command: &str) -> Result<String, String> {
    let mut root: serde_json::Value = if existing.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(existing)
            .map_err(|e| format!("the existing config didn't parse as JSON ({e}) — not touching it"))?
    };
    let obj = root
        .as_object_mut()
        .ok_or("the existing config isn't a JSON object — not touching it")?;
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let servers = servers
        .as_object_mut()
        .ok_or("'mcpServers' isn't an object — not touching it")?;
    servers.insert(
        "embral".into(),
        serde_json::json!({ "command": command }),
    );
    serde_json::to_string_pretty(&root).map_err(|e| e.to_string())
}

/// Remove `mcpServers.embral`; `Ok(None)` when it wasn't there.
fn remove_mcp_server(existing: &str) -> Result<Option<String>, String> {
    let mut root: serde_json::Value = serde_json::from_str(existing)
        .map_err(|e| format!("the existing config didn't parse as JSON ({e}) — not touching it"))?;
    let removed = root
        .get_mut("mcpServers")
        .and_then(|s| s.as_object_mut())
        .and_then(|s| s.remove("embral"))
        .is_some();
    if !removed {
        return Ok(None);
    }
    serde_json::to_string_pretty(&root)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// TOML literal strings need no backslash escaping and our paths hold no
/// single quotes.
fn codex_toml_snippet(server_path: &str) -> String {
    format!("[mcp_servers.embral]\ncommand = '{server_path}'\nargs = []")
}

fn json_registered(path: Option<&PathBuf>) -> bool {
    let Some(path) = path else { return false };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| v["mcpServers"]["embral"].is_object())
        .unwrap_or(false)
}

fn codex_registered() -> bool {
    codex_config()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.lines().any(|l| l.trim() == "[mcp_servers.embral]"))
        .unwrap_or(false)
}

// --- Running client CLIs ---

use crate::platform::find_cli;

/// Run a resolved CLI without flashing a console window, bounded so a hung
/// client can't wedge the settings page. Handing std/tokio the explicit
/// `.cmd` path is safe: the runtime wraps cmd.exe itself with correct
/// quoting, and our args are fixed strings plus a path.
async fn run_cli(exe: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.args(args).stdin(std::process::Stdio::null());
    crate::platform::hide_console_tokio(&mut cmd);
    tokio::time::timeout(Duration::from_secs(15), cmd.output())
        .await
        .map_err(|_| format!("{} timed out after 15 s", exe.display()))?
        .map_err(|e| format!("{}: {e}", exe.display()))
}

/// The last chunk of a failed CLI's chatter — enough to see why, short
/// enough for an inline error line.
fn output_tail(output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if text.is_empty() {
        text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    }
    if text.is_empty() {
        text = format!("exit status {}", output.status);
    }
    match text.char_indices().rev().nth(399) {
        Some((i, _)) => text[i..].to_string(),
        None => text,
    }
}

async fn run_registration(exe: &Path, args: &[&str], success: &str) -> Result<String, String> {
    let output = run_cli(exe, args).await?;
    if output.status.success() {
        Ok(success.to_string())
    } else {
        Err(output_tail(&output))
    }
}

// --- Commands ---

#[tauri::command]
pub async fn mcp_setup_info() -> Result<McpSetupInfo, String> {
    let (path, exists) = server_binary()?;
    let display = dunce_simplify(&path);
    let escaped = display.replace('\\', "\\\\");
    Ok(McpSetupInfo {
        claude_code_command: format!("claude mcp add --scope user embral -- \"{display}\""),
        config_json: format!(
            "{{\n  \"mcpServers\": {{\n    \"embral\": {{\n      \"command\": \"{escaped}\"\n    }}\n  }}\n}}"
        ),
        codex_command: format!("codex mcp add embral -- \"{display}\""),
        codex_toml: codex_toml_snippet(&display),
        claude_desktop_config_path: desktop_config_dir_on_disk()
            .map(|d| d.join(DESKTOP_CONFIG_FILE).to_string_lossy().to_string())
            .unwrap_or_default(),
        path: display,
        exists,
    })
}

#[tauri::command]
pub async fn mcp_clients_status() -> Result<McpClientsStatus, String> {
    let (server_path, server_exists) = server_binary()?;

    let (desktop_dir, desktop_installed) = resolve_desktop_config_dir().await;
    let desktop_config = desktop_dir.map(|d| d.join(DESKTOP_CONFIG_FILE));
    let claude_desktop = ClientStatus {
        installed: desktop_installed,
        registered: json_registered(desktop_config.as_ref()),
        detail: if desktop_installed {
            desktop_config
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            "Claude Desktop was not found on this machine".into()
        },
    };

    let (claude_cli, codex_cli) = tokio::join!(find_cli("claude"), find_cli("codex"));
    let claude_code = ClientStatus {
        installed: claude_cli.is_some(),
        registered: json_registered(claude_code_config().as_ref()),
        detail: claude_cli
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "the claude CLI was not found on PATH".into()),
    };
    let codex = ClientStatus {
        installed: codex_cli.is_some(),
        registered: codex_registered(),
        detail: codex_cli
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "the codex CLI was not found on PATH".into()),
    };

    Ok(McpClientsStatus {
        server_path: dunce_simplify(&server_path),
        server_exists,
        claude_desktop,
        claude_code,
        codex,
    })
}

/// The client's telemetry name — the vocabulary's closed set.
fn client_label(client: McpClient) -> &'static str {
    match client {
        McpClient::ClaudeDesktop => "claude_desktop",
        McpClient::ClaudeCode => "claude_code",
        McpClient::Codex => "codex",
    }
}

#[tauri::command]
pub async fn mcp_register(
    state: tauri::State<'_, crate::AppState>,
    client: McpClient,
) -> Result<String, String> {
    let result = mcp_register_inner(client).await;
    match &result {
        Ok(_) => crate::telemetry::track(
            &state,
            "mcp_registered",
            serde_json::json!({ "client": client_label(client) }),
        ),
        Err(_) => crate::telemetry::track(
            &state,
            "error",
            serde_json::json!({ "category": "mcp_register_failed" }),
        ),
    }
    result
}

async fn mcp_register_inner(client: McpClient) -> Result<String, String> {
    let (server_path, exists) = server_binary()?;
    if !exists {
        return Err(format!(
            "A part of embral this feature needs is missing (expected at {}) — reinstalling the app should fix this",
            dunce_simplify(&server_path)
        ));
    }
    let display = dunce_simplify(&server_path);

    match client {
        McpClient::ClaudeDesktop => {
            let (dir, installed) = resolve_desktop_config_dir().await;
            let dir = dir.ok_or("no config directory on this system")?;
            if !installed {
                return Err("Claude Desktop doesn't appear to be installed (no Claude config folder found)".into());
            }
            // create_dir_all covers an installed-but-never-launched MSIX
            // package, where Get-AppxPackage found it before the folder existed.
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let config = dir.join(DESKTOP_CONFIG_FILE);
            let existing = std::fs::read_to_string(&config).unwrap_or_default();
            let updated = upsert_mcp_server(&existing, &display)?;
            std::fs::write(&config, updated).map_err(|e| e.to_string())?;
            Ok("Registered — restart Claude Desktop to pick up embral".into())
        }
        McpClient::ClaudeCode => {
            let cli = find_cli("claude")
                .await
                .ok_or("the claude CLI was not found on PATH")?;
            // remove-then-add: `add` errors on an existing name, and this
            // also refreshes the path after a binary move.
            let _ = run_cli(&cli, &["mcp", "remove", "--scope", "user", "embral"]).await;
            run_registration(
                &cli,
                &["mcp", "add", "--scope", "user", "embral", "--", &display],
                "Registered with Claude Code for every project",
            )
            .await
        }
        McpClient::Codex => {
            let cli = find_cli("codex")
                .await
                .ok_or("the codex CLI was not found on PATH")?;
            let _ = run_cli(&cli, &["mcp", "remove", "embral"]).await;
            run_registration(
                &cli,
                &["mcp", "add", "embral", "--", &display],
                "Registered with Codex",
            )
            .await
        }
    }
}

#[tauri::command]
pub async fn mcp_unregister(
    state: tauri::State<'_, crate::AppState>,
    client: McpClient,
) -> Result<String, String> {
    let result = mcp_unregister_inner(client).await;
    if result.is_ok() {
        crate::telemetry::track(
            &state,
            "mcp_unregistered",
            serde_json::json!({ "client": client_label(client) }),
        );
    }
    result
}

async fn mcp_unregister_inner(client: McpClient) -> Result<String, String> {
    match client {
        McpClient::ClaudeDesktop => {
            let config = desktop_config_dir_on_disk()
                .map(|d| d.join(DESKTOP_CONFIG_FILE))
                .ok_or("no config directory on this system")?;
            let existing = std::fs::read_to_string(&config)
                .map_err(|_| "no Claude Desktop config file to edit")?;
            match remove_mcp_server(&existing)? {
                Some(updated) => {
                    std::fs::write(&config, updated).map_err(|e| e.to_string())?;
                    Ok("Removed — restart Claude Desktop to apply".into())
                }
                None => Ok("embral wasn't registered with Claude Desktop".into()),
            }
        }
        McpClient::ClaudeCode => {
            let cli = find_cli("claude")
                .await
                .ok_or("the claude CLI was not found on PATH")?;
            run_registration(
                &cli,
                &["mcp", "remove", "--scope", "user", "embral"],
                "Removed from Claude Code",
            )
            .await
        }
        McpClient::Codex => {
            let cli = find_cli("codex")
                .await
                .ok_or("the codex CLI was not found on PATH")?;
            run_registration(&cli, &["mcp", "remove", "embral"], "Removed from Codex").await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_preserves_everything_else() {
        let existing = r#"{
            "theme": "dark",
            "mcpServers": {
                "other": { "command": "other.exe", "args": ["-x"] }
            },
            "unknownFuture": [1, 2]
        }"#;
        let updated = upsert_mcp_server(existing, r"C:\apps\embral-mcp.exe").unwrap();
        let v: serde_json::Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["unknownFuture"][1], 2);
        assert_eq!(v["mcpServers"]["other"]["args"][0], "-x");
        assert_eq!(v["mcpServers"]["embral"]["command"], r"C:\apps\embral-mcp.exe");
    }

    /// Linux port check, run manually because it reads the developer's real
    /// `~/.claude.json`:
    /// `cargo test -p embral --lib linux_registration_probe -- --ignored --nocapture`.
    ///
    /// Registration on Linux has three moving parts the unit tests above
    /// cannot cover, because they all touch the machine: the sidecar has to
    /// resolve, `claude` has to be findable on a PATH a desktop launch
    /// inherits, and the merge has to survive a *real* config — Claude Code's
    /// own file is tens of kilobytes of nested project state, not the tidy
    /// fixtures above. Nothing is written; the merge runs in memory and every
    /// top-level key is checked to survive it.
    #[test]
    #[ignore = "manual probe; reads the real ~/.claude.json (never writes)"]
    fn linux_registration_probe() {
        let (bin, exists) = server_binary().expect("sidecar path resolves");
        eprintln!("sidecar: {} (present: {exists})", bin.display());
        assert!(exists, "stage the sidecar first: node scripts/prepare-sidecar.mjs --debug");
        assert_eq!(bin.file_name().unwrap(), "embral-mcp", "no .exe suffix on Linux");

        let claude = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(crate::platform::find_cli("claude"));
        eprintln!("claude: {claude:?}");
        assert!(claude.is_some(), "find_cli must resolve a real claude");

        let path = claude_code_config().expect("home dir");
        let before = std::fs::read_to_string(&path).expect("read the real config");
        eprintln!("config: {} ({} bytes)", path.display(), before.len());

        let after = upsert_mcp_server(&before, bin.to_str().unwrap()).expect("merge");
        let a: serde_json::Value = serde_json::from_str(&before).unwrap();
        let b: serde_json::Value = serde_json::from_str(&after).unwrap();

        // Every pre-existing top-level key survives, byte-identical.
        for (k, v) in a.as_object().unwrap() {
            if k == "mcpServers" {
                continue;
            }
            assert_eq!(b.get(k), Some(v), "top-level key {k:?} was altered");
        }
        // Any MCP server already registered is left alone.
        if let Some(existing) = a.get("mcpServers").and_then(|m| m.as_object()) {
            for (k, v) in existing {
                if k != "embral" {
                    assert_eq!(b["mcpServers"].get(k), Some(v), "clobbered server {k:?}");
                }
            }
        }
        assert_eq!(b["mcpServers"]["embral"]["command"], bin.to_str().unwrap());
        eprintln!(
            "merge preserved {} top-level keys and added mcpServers.embral",
            a.as_object().unwrap().len()
        );
    }

    #[test]
    fn upsert_handles_empty_and_updates_in_place() {
        let fresh = upsert_mcp_server("", "a.exe").unwrap();
        let v: serde_json::Value = serde_json::from_str(&fresh).unwrap();
        assert_eq!(v["mcpServers"]["embral"]["command"], "a.exe");

        let moved = upsert_mcp_server(&fresh, "b.exe").unwrap();
        let v: serde_json::Value = serde_json::from_str(&moved).unwrap();
        assert_eq!(v["mcpServers"]["embral"]["command"], "b.exe");
    }

    #[test]
    fn malformed_config_is_refused_untouched() {
        assert!(upsert_mcp_server("{ not json", "a.exe").is_err());
        assert!(upsert_mcp_server("[1,2,3]", "a.exe").is_err());
        assert!(remove_mcp_server("{ not json").is_err());
    }

    #[test]
    fn remove_takes_only_embral() {
        let existing = r#"{"mcpServers": {"embral": {"command": "e.exe"}, "other": {"command": "o.exe"}}}"#;
        let updated = remove_mcp_server(existing).unwrap().unwrap();
        let v: serde_json::Value = serde_json::from_str(&updated).unwrap();
        assert!(v["mcpServers"]["embral"].is_null());
        assert_eq!(v["mcpServers"]["other"]["command"], "o.exe");

        assert!(remove_mcp_server(r#"{"mcpServers": {}}"#).unwrap().is_none());
        assert!(remove_mcp_server("{}").unwrap().is_none());
    }

    #[test]
    fn codex_snippet_keeps_windows_paths_literal() {
        let toml = codex_toml_snippet(r"C:\Program Files\embral\embral-mcp.exe");
        assert!(toml.contains("[mcp_servers.embral]"));
        assert!(toml.contains(r"command = 'C:\Program Files\embral\embral-mcp.exe'"));
    }
}
