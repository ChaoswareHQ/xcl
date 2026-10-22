//! Runs one script many times in one process and reports the best time.
//!
//! Process start-up dominates a single short run, so the cross-language harness
//! subtracts it; here the VM is reused across iterations, which leaves only the
//! work under test. Use it to compare two builds or two variants of one script.
//!
//! ```sh
//! cargo run -q --release -p xcl-vm --example time_script -- bench.xcl 20
//! ```

use std::time::Instant;

use xcl_vm::VM;
use xcl_vm::bytecode::compile;
use xcl_vm::compiler::lexer::lex;
use xcl_vm::compiler::parser::parse;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: time_script <file.xcl> [runs]");
        std::process::exit(2);
    };
    let runs: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(20);
    let src = std::fs::read_to_string(&path).expect("read script");

    let tokens = lex(&src, &path);
    let ast = parse(&tokens, &src);
    let compile_start = Instant::now();
    let bc = compile(&ast);
    let compile_ms = compile_start.elapsed().as_secs_f64() * 1e3;
    let build_ms = compile_ms;

    let mut vm = VM::new();
    let mut best = f64::INFINITY;
    let mut checksum = 0i64;
    for _ in 0..runs {
        let start = Instant::now();
        checksum = vm.run(&bc).as_i48();
        let elapsed = start.elapsed().as_secs_f64() * 1e3;
        if elapsed < best {
            best = elapsed;
        }
    }
    println!("{path}: {best:.3} ms best of {runs}, checksum {checksum}");
    if std::env::var_os("XCL_TIME_COMPILE").is_some() {
        println!("  compile: {build_ms:.3} ms");
    }
    #[cfg(feature = "opcode-stats")]
    vm.print_opcode_histogram(10);
}
