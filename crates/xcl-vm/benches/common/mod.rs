//! Common benchmark harness for pure-VM benchmarks.
//!
//! Compiles XCL bytecode once, validates against Luau, then benchmarks
//! execution only (VM setup excluded from measurement).

#![allow(dead_code)]

use criterion::{BatchSize, Criterion, BenchmarkGroup};
use criterion::measurement::Measurement;
use std::time::Duration;
use xcl_vm::{Bytecode, Value, VM};
use xcl_vm::compiler::{lexer, parser};
use xcl_vm::bytecode;

/// Compile XCL source to bytecode using the public pipeline.
pub fn compile_xcl(src: &str) -> Bytecode {
    let tokens = lexer::lex(src, "<bench>");
    let ast = parser::parse(&tokens, src);
    bytecode::compile(&ast)
}

/// Run precompiled bytecode on a fresh VM and return the result.
pub fn run_xcl(bc: &Bytecode) -> Value {
    let mut vm = VM::new();
    vm.run(bc)
}

/// Extract an f64 from a Value, panicking with a clear message if not numeric.
pub fn as_number(v: Value) -> f64 {
    if v.is_float() {
        v.as_f64()
    } else if v.is_int() {
        v.as_i48() as f64
    } else {
        panic!(
            "as_number: expected int or float, got {:?}",
            v.as_string()
        );
    }
}

/// Check if a value is an integer result (for exact equality checks).
fn is_integer_workload(expected: f64) -> bool {
    expected.fract() == 0.0 && expected.abs() < (1i64 << 47) as f64
}

/// Validate that `actual` matches `expected` within tolerance.
fn validate(actual: f64, expected: f64, name: &str) {
    if is_integer_workload(expected) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "[bench] {} validation failed: expected {} (exact), got {}",
            name,
            expected,
            actual
        );
    } else {
        let rel = if expected.abs() > 1e-15 {
            (actual - expected).abs() / expected.abs()
        } else {
            (actual - expected).abs()
        };
        assert!(
            rel <= 1e-6,
            "[bench] {} validation failed: expected {}, got {} (rel error {:.2e})",
            name,
            expected,
            actual,
            rel
        );
    }
}

/// Print the setup line showing instruction counts.
fn print_setup(name: &str, bc: &Bytecode) {
    println!(
        "[bench] {}: instrs={} wide={} strings={}",
        name,
        bc.instructions.len(),
        bc.wide_operands.len(),
        bc.string_table.len()
    );
}

/// Benchmark a pair of XCL and Luau workloads, validating both against the expected value.
pub fn bench_pair<M: Measurement>(
    group: &mut BenchmarkGroup<'_, M>,
    name: &str,
    xcl_src: &str,
    luau_src: &str,
    expected: f64,
) {
    // Compile XCL once
    let bc = compile_xcl(xcl_src);

    // Compile Luau once — wrap in a function that returns the result
    let lua = mlua::Lua::new();
    let luau_fn_src = format!("return (function() {} end)", luau_src);
    let func: mlua::Function = lua
        .load(&luau_fn_src)
        .eval()
        .expect("Luau compile failed");

    // Validate XCL
    let xcl_result = run_xcl(&bc);
    let xcl_num = as_number(xcl_result);
    validate(xcl_num, expected, &format!("{} (xcl)", name));

    // Validate Luau
    let luau_result: f64 = func.call(()).expect("Luau run failed");
    validate(luau_result, expected, &format!("{} (luau)", name));

    // Print setup line
    print_setup(name, &bc);

    // Benchmark XCL — fresh VM per iteration, setup excluded
    group.bench_function(format!("{}/xcl", name), |b| {
        b.iter_batched(
            VM::new,
            |mut vm| std::hint::black_box(vm.run(std::hint::black_box(&bc))),
            BatchSize::SmallInput,
        );
    });

    // Benchmark Luau — precompiled function call
    group.bench_function(format!("{}/luau", name), |b| {
        b.iter(|| std::hint::black_box(func.call::<f64>(()).unwrap()))
    });
}

/// Benchmark a pair of string-concatenation workloads.
pub fn bench_pair_string<M: Measurement>(
    group: &mut BenchmarkGroup<'_, M>,
    name: &str,
    xcl_src: &str,
    luau_src: &str,
    expected_len: usize,
) {
    // Compile XCL once
    let bc = compile_xcl(xcl_src);

    // Compile Luau once
    let lua = mlua::Lua::new();
    let luau_fn_src = format!("return (function() {} end)", luau_src);
    let func: mlua::Function = lua
        .load(&luau_fn_src)
        .eval()
        .expect("Luau compile failed");

    // Validate XCL string length
    let xcl_result = run_xcl(&bc);
    let xcl_str = xcl_result
        .as_str()
        .expect("XCL string result expected");
    assert_eq!(
        xcl_str.len(),
        expected_len,
        "[bench] {} (xcl) string length: expected {}, got {}",
        name,
        expected_len,
        xcl_str.len()
    );

    // Validate Luau string length
    let luau_result: mlua::LuaString = func.call(()).expect("Luau run failed");
    let luau_str = luau_result.to_str().expect("Luau string");
    assert_eq!(
        luau_str.len(),
        expected_len,
        "[bench] {} (luau) string length: expected {}, got {}",
        name,
        expected_len,
        luau_str.len()
    );

    // Print setup line
    print_setup(name, &bc);

    // Benchmark XCL
    group.bench_function(format!("{}/xcl", name), |b| {
        b.iter_batched(
            VM::new,
            |mut vm| std::hint::black_box(vm.run(std::hint::black_box(&bc))),
            BatchSize::SmallInput,
        );
    });

    // Benchmark Luau
    group.bench_function(format!("{}/luau", name), |b| {
        b.iter(|| {
            let s: mlua::LuaString = func.call(()).unwrap();
            std::hint::black_box(s.to_str())
        })
    });
}

/// Benchmark only the compile step (lex + parse + compile) repeatedly.
pub fn bench_compile_only<M: Measurement>(group: &mut BenchmarkGroup<'_, M>, name: &str, src: &str) {
    group.bench_function(name.to_string(), |b| {
        b.iter(|| {
            let tokens = lexer::lex(src, "<bench>");
            let ast = parser::parse(&tokens, src);
            std::hint::black_box(bytecode::compile(&ast))
        })
    });
}

/// Benchmark compile + run via evaluate(), validating once.
pub fn bench_compile_and_run<M: Measurement>(group: &mut BenchmarkGroup<'_, M>, name: &str, src: &str, expected: f64) {
    // Validate once
    let mut vm = VM::new();
    let result = xcl_vm::evaluate(src, &mut vm);
    let num = as_number(result);
    validate(num, expected, name);

    // Benchmark evaluate repeatedly
    group.bench_function(name.to_string(), |b| {
        b.iter(|| {
            let mut vm = VM::new();
            std::hint::black_box(xcl_vm::evaluate(std::hint::black_box(src), &mut vm))
        })
    });
}

/// Criterion config used by all benchmark files.
pub fn criterion_config() -> Criterion {
    Criterion::default()
        .sample_size(30)
        .warm_up_time(Duration::from_millis(300))
        .measurement_time(Duration::from_secs(2))
}
