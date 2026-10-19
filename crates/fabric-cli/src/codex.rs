//! OpenAI Codex integration: registers Fabric's MCP server in Codex's `config.toml`
//! (`$CODEX_HOME`, default `~/.codex`), preserving everything else in the file.
//!
//! Codex has no per-tool hooks, so its steps are recorded when it calls the
//! `fabric_checkpoint` tool (the MCP server's instructions ask it to), or with
//! `fabric checkpoint --agent codex`.

use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde_json::json;
use toml_edit::{Array, DocumentMut, Item, Table, value};

use crate::error::{CliError, exit};
use crate::home::{display_path, write_durably};

pub fn integrate(json_output: bool) -> Result<u8, CliError> {
    let codex_home = match env::var_os("CODEX_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => dirs::home_dir()
            .ok_or_else(|| {
                CliError::new(
                    exit::ERROR,
                    "no_home",
                    "cannot find your home directory; set CODEX_HOME",
                )
            })?
            .join(".codex"),
    };
    let path = codex_home.join("config.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(CliError::io(&path, e)),
    };
    let mut doc: DocumentMut = text.parse().map_err(|e| {
        CliError::new(
            exit::ERROR,
            "invalid_settings",
            format!(
                "{} is not valid TOML ({e}); fix it and try again — it was not changed",
                display_path(&path)
            ),
        )
    })?;

    let exe = crate::claude::fabric_exe()?;
    let servers = doc
        .entry("mcp_servers")
        .or_insert_with(|| {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        })
        .as_table_mut()
        .ok_or_else(|| {
            CliError::new(
                exit::ERROR,
                "invalid_settings",
                format!(
                    "`mcp_servers` in {} is not a table; it was not changed",
                    display_path(&path)
                ),
            )
        })?;

    let already = servers
        .get("fabric")
        .and_then(Item::as_table)
        .is_some_and(|t| {
            t.get("command").and_then(Item::as_str) == Some(exe.as_str())
                && t.get("args").and_then(Item::as_array).is_some_and(|a| {
                    a.len() == 1 && a.get(0).and_then(|v| v.as_str()) == Some("mcp")
                })
        });
    if !already {
        let mut server = Table::new();
        server.insert("command", value(exe.clone()));
        let mut args = Array::new();
        args.push("mcp");
        server.insert("args", value(args));
        servers.insert("fabric", Item::Table(server));
        fs::create_dir_all(&codex_home).map_err(|e| CliError::io(&codex_home, e))?;
        write_durably(&path, doc.to_string().as_bytes())?;
    }

    let shown = display_path(&path);
    if json_output {
        let out = json!({
            "schema": "fabric.integrate/v1",
            "agent": "codex",
            "settings": shown,
            "changed": !already,
            "mcp_server": { "command": exe, "args": ["mcp"] },
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&out).expect("output always serializes")
        );
    } else if already {
        println!("Codex is already connected to Fabric ({shown})");
    } else {
        println!(
            "Codex can now ask Fabric what other agents did (MCP server added to {shown}).\nRestart Codex to pick it up. Codex records its own steps through the fabric_checkpoint tool."
        );
    }
    Ok(exit::OK)
}
