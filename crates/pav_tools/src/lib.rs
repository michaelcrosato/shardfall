//! Agent layer: one tool registry, exposed through the `pav` CLI (one-shot or REPL), an
//! MCP stdio server, and a live bridge into the running game. Every tool works on a `Session`
//! (a simulation plus camera/view state and an optional headless GPU for captures).

pub mod agent_tools;
pub mod anim_tools;
pub mod animation_tools;
pub mod asset_preview_tools;
pub mod asset_tools;
pub mod bridge;
pub mod game_tools;
pub mod live_feedback;
pub mod mcp;
pub mod mocap;
pub mod preview_tools;
pub mod session;
pub mod tools;

pub use session::Session;
pub use tools::{Output, TOOLS, Tool, call};
