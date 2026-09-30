//! `pav` — the headless agent CLI.
//!
//!   pav <tool> [key=value ...]      run one tool on a fresh session (scene=... seed=... ticks=... apply first)
//!   pav repl                        read tool lines from stdin, one session for all of them
//!   pav help                        list tools

use std::io::BufRead;

use anyhow::Result;
use pav_tools::tools::{self, Args, Output};
use pav_tools::{Session, TOOLS};
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

fn print(out: Result<Output>) -> bool {
    match out {
        Ok(Output::Json(v)) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            true
        }
        Ok(Output::Image { meta, .. }) => {
            println!("{}", serde_json::to_string_pretty(&meta).unwrap_or_default());
            true
        }
        Err(e) => {
            println!("{}", serde_json::json!({ "error": format!("{e:#}") }));
            false
        }
    }
}

fn help() {
    println!("pav — Pavilion agent CLI\n\nUsage: pav <tool> [key=value ...] | pav repl | pav help\n\nTools:");
    for t in TOOLS {
        let args: Vec<String> = t.args.iter().map(|a| format!("{}=<{}>", a.name, a.kind)).collect();
        println!("  {:<10} {}  {}", t.name, t.help, args.join(" "));
    }
    println!("\nOne-shot calls accept scene=, seed= and ticks= to set up the session before the tool runs.");
}

fn main() -> Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = argv.first().cloned() else {
        help();
        return Ok(());
    };
    match cmd.as_str() {
        "help" | "--help" | "-h" => help(),
        "repl" => {
            let mut session = Session::new("test", 1)?;
            for line in std::io::stdin().lock().lines() {
                let line = line?;
                let words: Vec<String> = line.split_whitespace().map(String::from).collect();
                let Some(name) = words.first() else { continue };
                if name.starts_with('#') {
                    continue;
                }
                print(tools::call(&mut session, name, &parse_args(&words[1..])));
            }
        }
        name => {
            let args = parse_args(&argv[1..]);
            let scene = args.get("scene").and_then(|v| v.as_str()).unwrap_or("test").to_string();
            let seed = args.get("seed").and_then(|v| v.as_u64()).unwrap_or(1);
            let mut session = Session::new(&scene, seed)?;
            if name != "step" && name != "bench" {
                if let Some(t) = args.get("ticks").and_then(|v| v.as_u64()) {
                    session.step(t);
                }
            }
            if !print(tools::call(&mut session, name, &args)) {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}
