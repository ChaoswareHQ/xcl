//! Prints every playground view for one source string.
//!
//! Used while building the site: the JSON printed here is exactly what the page
//! receives from the wasm module.
//!
//! ```sh
//! cargo run -p xcl-wasm --example dump -- '1 + 2'
//! ```

use std::env;

fn main() {
    let source = env::args().skip(1).collect::<Vec<_>>().join(" ");
    let source = if source.is_empty() {
        "1 + 2".to_string()
    } else {
        source
    };

    println!("== diagnose ==\n{}", xcl_wasm::diagnose(&source));
    println!("== decompile ==\n{}", xcl_wasm::decompile(&source));
    println!("== program ==\n{}", xcl_wasm::program(&source));
    println!("== run ==\n{}", xcl_wasm::run(&source));

    let map = xcl_wasm::opcode_map();
    let head = &map[..map.len().min(320)];
    println!("== opcode_map (truncated) ==\n{head}...");
}
