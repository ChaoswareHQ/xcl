//! Minimal Luau interpreter runner used by `scripts/crosslang`.
//!
//! This is the Luau *interpreter* (the same backend the criterion harness
//! uses). Luau has no JIT; its optional native codegen is an AOT backend and is
//! not enabled in this build.

use mlua::Lua;
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: luau-runner <script.lua>");
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
