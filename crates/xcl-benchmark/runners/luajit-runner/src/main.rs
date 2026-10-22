//! Minimal LuaJIT runner used by `scripts/crosslang` for cross-language timing.
//!
//! Not part of the xcl workspace: mlua's `luajit` and `luau` backends cannot be
//! enabled in the same dependency graph.

use mlua::Lua;
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: luajit-runner <script.lua>");
        return ExitCode::FAILURE;
    };
    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let lua = Lua::new();
    match lua.load(&src).set_name(&path).exec() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{path}: {e}");
            ExitCode::FAILURE
        }
    }
}
