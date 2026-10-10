//! Minimal MCP server (JSON-RPC 2.0 over stdio, newline-delimited). Exposes every tool in the
//! registry; images come back as MCP image content so agents can look at captures directly.

use std::io::{BufRead, Write};

use anyhow::Result;
use serde_json::{Value, json};

use crate::bridge::Client;
use crate::session::Session;
use crate::tools::{self, Args, Output, TOOLS};

const PROTOCOL: &str = "2025-06-18";

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn tool_list() -> Value {
    json!({ "tools": TOOLS.iter().map(|t| json!({
        "name": t.name,
        "description": t.help,
        "inputSchema": tools::schema(t),
    })).collect::<Vec<_>>() })
}

fn call(session: &mut Session, name: &str, args: &Args) -> Value {
    match tools::call(session, name, args) {
        Ok(Output::Json(v)) => json!({ "content": [{ "type": "text", "text": v.to_string() }] }),
        Ok(Output::Image { png, meta, .. }) => json!({ "content": [
            { "type": "image", "data": base64(&png), "mimeType": "image/png" },
            { "type": "text", "text": meta.to_string() },
        ] }),
        Err(e) => json!({ "content": [{ "type": "text", "text": format!("{e:#}") }], "isError": true }),
    }
}

/// Forwards a call to a running game through the live bridge.
fn call_live(client: &mut Client, name: &str, args: &Args) -> Value {
    match client.request(name, args) {
        Ok(r) => {
            if let Some(e) = r.get("error").and_then(|e| e.as_str()) {
                return json!({ "content": [{ "type": "text", "text": e }], "isError": true });
            }
            let text = r.get("result").cloned().unwrap_or(Value::Null).to_string();
            match r.get("png").and_then(|p| p.as_str()) {
                Some(png) => json!({ "content": [
                    { "type": "image", "data": png, "mimeType": "image/png" },
                    { "type": "text", "text": text },
                ] }),
                None => json!({ "content": [{ "type": "text", "text": text }] }),
            }
        }
        Err(e) => json!({ "content": [{ "type": "text", "text": format!("{e:#}") }], "isError": true }),
    }
}

/// Serves a local headless session until stdin closes.
pub fn serve(mut session: Session) -> Result<()> {
    run(&mut |name, args| call(&mut session, name, args), "headless session")
}

/// Serves the running game at `addr` (live bridge) until stdin closes.
pub fn serve_live(addr: &str) -> Result<()> {
    let mut client = Client::connect(addr)?;
    run(&mut |name, args| call_live(&mut client, name, args), "the running game (live bridge)")
}

fn run(call: &mut dyn FnMut(&str, &Args) -> Value, target: &str) -> Result<()> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}}))?;
                out.flush()?;
                continue;
            }
        };
        let Some(id) = msg.get("id").cloned() else { continue }; // notifications need no reply
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or(PROTOCOL),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "shardfall", "version": env!("CARGO_PKG_VERSION") },
                "instructions": format!("Pavilion game engine tools, connected to {target}. Start with `scenes` and `load` (or `rooms` and `goto`), drive the player with `input`, look with `capture`. Parameters: `params` / `set`. Animation: use `clips` to find sources, `anim_edit` to create/copy/inspect/edit, and `anim_preview` to play/pause/scrub. Edits save automatically and appear in the live studio; pass if_revision from the last result. `capture`, `filmstrip`, and `animsheet` return images. The live studio is started with `shardfall --animation-studio`; connect this MCP server with `pav mcp --live`."),
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(tool_list()),
            "tools/call" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").and_then(|v| v.as_object()).cloned().unwrap_or_default();
                Ok(call(name, &args))
            }
            _ => Err(json!({ "code": -32601, "message": format!("unknown method {method}") })),
        };
        let reply = match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
        };
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_known_values() {
        assert_eq!(super::base64(b"Man"), "TWFu");
        assert_eq!(super::base64(b"Ma"), "TWE=");
        assert_eq!(super::base64(b"M"), "TQ==");
    }
}
