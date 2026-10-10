//! `pav` — the headless agent CLI.
//!
//!   pav <tool> [key=value ...]      run one tool on a fresh session (scene=... seed=... ticks=... apply first)
//!   pav repl [--stop-on-error]       read tool lines from stdin, one session for all of them
//!   pav live [addr] [--stop-on-error] the same, but inside a running game (started with --bridge)
//!   pav mcp [scene]                 MCP server on a headless session
//!   pav mcp --live [addr]           MCP server forwarding to a running game
//!   pav help                        list tools

use std::io::BufRead;

use anyhow::{Result, anyhow};
use pav_tools::tools::{self, Args, Output};
use pav_tools::{Session, TOOLS, bridge};
use serde_json::Value;

fn parse_args(words: &[String]) -> Args {
    let mut a = Args::new();
    for w in words {
        if let Some((k, v)) = w.split_once('=') {
            let val = serde_json::from_str::<Value>(v).unwrap_or_else(|_| Value::String(v.to_string()));
            a.insert(k.trim_start_matches("--").to_string(), val);
        }
    }
    a
}

/// Structured lines keep animation poses and text intact in a REPL. The older
/// `tool key=value` syntax remains useful for short commands.
fn parse_line(line: &str) -> Result<Option<(String, Args)>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    if line.starts_with('{') {
        let value: Value = serde_json::from_str(line)?;
        let name = value
            .get("tool")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("a JSON command needs a non-empty tool string"))?;
        let args = match value.get("args") {
            Some(Value::Object(a)) => a.clone(),
            None => Args::new(),
            _ => return Err(anyhow!("a JSON command's args must be an object")),
        };
        return Ok(Some((name.to_string(), args)));
    }
    let words: Vec<String> = line.split_whitespace().map(String::from).collect();
    Ok(Some((words[0].clone(), parse_args(&words[1..]))))
}

fn print(out: Result<Output>) -> bool {
    match out {
        Ok(Output::Json(v)) => {
            println!("{}", serde_json::to_string(&v).unwrap_or_default());
            true
        }
        Ok(Output::Image { meta, .. }) => {
            println!("{}", serde_json::to_string(&meta).unwrap_or_default());
            true
        }
        Err(e) => {
            println!("{}", serde_json::json!({ "error": format!("{e:#}") }));
            false
        }
    }
}

fn run_lines(input: impl BufRead, stop_on_error: bool, mut call: impl FnMut(&str, &Args) -> Result<Output>) -> Result<()> {
    for (line_number, line) in input.lines().enumerate() {
        let ok = match parse_line(&line?) {
            Ok(Some((name, args))) => print(call(&name, &args)),
            Ok(None) => true,
            Err(e) => print(Err(e)),
        };
        if !ok && stop_on_error {
            return Err(anyhow!("command on line {} failed; remaining commands were not run", line_number + 1));
        }
    }
    Ok(())
}

fn help() {
    println!(
        "pav — Shardfall agent CLI\n\nUsage: pav <tool> [key=value ...] | pav repl [--stop-on-error] | pav live [addr] [--stop-on-error] | pav mcp [scene] | pav mcp --live [addr] | pav help\n\nTools:"
    );
    for t in TOOLS {
        let args: Vec<String> = t.args.iter().map(|a| format!("{}=<{}>", a.name, a.kind)).collect();
        println!("  {:<10} {}  {}", t.name, t.help, args.join(" "));
    }
    println!("\nOne-shot calls accept scene=, seed= and ticks= to set up the session before the tool runs.");
    println!("REPLs also accept JSON lines: {{\"tool\":\"anim_preview\",\"args\":{{\"clip\":\"QUATERNIUS/Idle_Loop\"}}}}");
    println!("Use --stop-on-error with REPL input files to stop before later commands can change data after a failed command.");
    println!("For live animation editing, start shardfall --animation-studio, then use pav mcp --live or pav live.");
}

fn main() -> Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = argv.first().cloned() else {
        help();
        return Ok(());
    };
    match cmd.as_str() {
        "help" | "--help" | "-h" => help(),
        "mcp" if argv.get(1).is_some_and(|a| a == "--live") => {
            pav_tools::mcp::serve_live(argv.get(2).map(String::as_str).unwrap_or(bridge::DEFAULT_ADDR))?;
        }
        "mcp" => {
            let scene = argv.get(1).cloned().unwrap_or_else(|| "playground".into());
            pav_tools::mcp::serve(Session::new(&scene, 1)?)?;
        }
        "live" => {
            let stop_on_error = argv.iter().any(|a| a == "--stop-on-error");
            let addr =
                argv.iter().skip(1).find(|a| a.as_str() != "--stop-on-error").map(String::as_str).unwrap_or(bridge::DEFAULT_ADDR);
            let mut client = bridge::Client::connect(addr)?;
            run_lines(std::io::stdin().lock(), stop_on_error, |name, args| client.call(name, args).map(Output::Json))?;
        }
        "repl" => {
            let mut session = Session::new("playground", 1)?;
            let stop_on_error = argv.iter().any(|a| a == "--stop-on-error");
            run_lines(std::io::stdin().lock(), stop_on_error, |name, args| tools::call(&mut session, name, args))?;
        }
        name => {
            let args = parse_args(&argv[1..]);
            let scene = args.get("scene").and_then(|v| v.as_str()).unwrap_or("playground").to_string();
            let seed = args.get("seed").and_then(|v| v.as_u64()).unwrap_or(1);
            let mut session = Session::new(&scene, seed)?;
            if name != "step" && name != "bench" {
                if let Some(t) = args.get("ticks").and_then(|v| v.as_u64()) {
                    session.step(t);
                }
            }
            let output = tools::call(&mut session, name, &args)
                .and_then(|output| pav_tools::creature_tools::finish_one_shot(&mut session, output));
            if !print(output) {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_repl_preserves_animation_arrays_and_spaces() {
        let (tool, args) = parse_line(
            r#"{"tool":"anim_edit","args":{"action":"key","name":"WORKSHOP/Slow Wave","pose":{"armR":[80, 20, 50, 25, 0]}}}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(tool, "anim_edit");
        assert_eq!(args["name"], "WORKSHOP/Slow Wave");
        assert_eq!(args["pose"]["armR"][2], 50);
    }

    #[test]
    fn repl_rejects_invalid_json_without_losing_next_command() {
        assert!(parse_line(r#"{"tool":"anim_edit","args":[]}"#).is_err());
        assert!(parse_line(r#"{"args":{}}"#).is_err());
        assert!(parse_line("  # a comment").unwrap().is_none());
        let (tool, args) = parse_line("anim_preview time=0.5 playing=false").unwrap().unwrap();
        assert_eq!(tool, "anim_preview");
        assert_eq!(args["time"], 0.5);
        assert_eq!(args["playing"], false);
    }

    #[test]
    fn batch_can_stop_before_a_following_mutation() {
        for stop_on_error in [false, true] {
            let mut calls = Vec::new();
            let result = run_lines(&b"create\nedit\n"[..], stop_on_error, |name, _| {
                calls.push(name.to_string());
                if name == "create" { Err(anyhow!("already exists")) } else { Ok(Output::Json(Value::Null)) }
            });
            assert_eq!(result.is_err(), stop_on_error);
            assert_eq!(calls.len(), if stop_on_error { 1 } else { 2 });
        }
    }
}
