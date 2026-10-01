//! Live bridge: the same tools, run inside the running game over a local TCP socket.
//!
//! Start the game with `--bridge` (or `bridge = "127.0.0.1:7878"` in `pavilion.toml`), then talk
//! to it with `pav live` (a REPL), `pav mcp --live` (MCP), or any client that writes one JSON
//! object per line:
//!
//!   -> {"id": 1, "tool": "capture", "args": {"width": 640}}
//!   <- {"id": 1, "result": {...}, "png": "<base64>"}     (png only for image tools)
//!   <- {"id": 1, "error": "..."}
//!
//! The tool `tools` lists every tool with its arguments.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::session::Session;
use crate::tools::{self, Args, Output, TOOLS};

pub const DEFAULT_ADDR: &str = "127.0.0.1:7878";

/// Every tool with its help and argument schema.
pub fn tool_list() -> Value {
    Value::Array(
        TOOLS.iter().map(|t| json!({ "name": t.name, "description": t.help, "inputSchema": tools::schema(t) })).collect(),
    )
}

/// Runs one request on a session and builds the reply line.
pub fn handle(session: &mut Session, req: &Value) -> Value {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let tool = req.get("tool").and_then(|v| v.as_str()).unwrap_or("");
    let args = req.get("args").and_then(|v| v.as_object()).cloned().unwrap_or_default();
    if tool == "tools" {
        return json!({ "id": id, "result": tool_list() });
    }
    match tools::call(session, tool, &args) {
        Ok(Output::Json(v)) => json!({ "id": id, "result": v }),
        Ok(Output::Image { png, meta, .. }) => json!({ "id": id, "result": meta, "png": crate::mcp::base64(&png) }),
        Err(e) => json!({ "id": id, "error": format!("{e:#}") }),
    }
}

/// A connection to a running game.
pub struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    next: u64,
}

impl Client {
    pub fn connect(addr: &str) -> Result<Self> {
        let writer = TcpStream::connect(addr).with_context(|| format!("no game listening on {addr} (start it with --bridge)"))?;
        writer.set_read_timeout(Some(Duration::from_secs(120)))?;
        let reader = BufReader::new(writer.try_clone()?);
        Ok(Self { reader, writer, next: 1 })
    }

    /// Sends one request and returns the whole reply object.
    pub fn request(&mut self, tool: &str, args: &Args) -> Result<Value> {
        let id = self.next;
        self.next += 1;
        writeln!(self.writer, "{}", json!({ "id": id, "tool": tool, "args": args }))?;
        self.writer.flush()?;
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(anyhow!("the game closed the connection"));
        }
        Ok(serde_json::from_str(&line)?)
    }

    /// Calls a tool: its result, or its error.
    pub fn call(&mut self, tool: &str, args: &Args) -> Result<Value> {
        let r = self.request(tool, args)?;
        match r.get("error").and_then(|e| e.as_str()) {
            Some(e) => Err(anyhow!("{e}")),
            None => Ok(r.get("result").cloned().unwrap_or(Value::Null)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn client_round_trip() {
        // A stand-in for the game: one session answering one connection.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut out = stream.try_clone().unwrap();
            let mut s = Session::new("empty", 1).unwrap();
            for line in BufReader::new(stream).lines() {
                let reply = handle(&mut s, &serde_json::from_str(&line.unwrap()).unwrap());
                writeln!(out, "{reply}").unwrap();
            }
        });
        let mut c = Client::connect(&addr).unwrap();
        let st = c.call("step", &serde_json::from_str(r#"{"ticks": 5}"#).unwrap()).unwrap();
        assert_eq!(st["tick"], 5);
        assert!(c.call("tools", &Args::new()).unwrap().as_array().unwrap().len() > 20);
        assert!(c.call("no_such_tool", &Args::new()).is_err());
        drop(c);
        server.join().unwrap();
    }
}
