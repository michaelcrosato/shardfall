//! `pav`: the Pavilion Lite command line (agent tools, REPL, MCP server, play window).
mod games;

fn main() {
    pavlite::tools::main(games::GAMES);
}
