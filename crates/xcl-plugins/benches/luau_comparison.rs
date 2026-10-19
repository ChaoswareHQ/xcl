//! Benchmark comparison: XCL vs Luau arithmetic performance.
//!
//! Each benchmark runs the same arithmetic workload in both XCL and Luau
//! to compare interpreter performance.

use criterion::{Criterion, criterion_group, criterion_main, BenchmarkGroup};
use std::hint::black_box;
use xcl_vm::evaluate;

/// Setup a fresh Luau VM instance
fn luau_vm() -> mlua::Lua {
    mlua::Lua::new()
}

/// Tight arithmetic loop: sum from 0 to 10000
fn bench_arithmetic_loop(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("arithmetic_loop_10k");

    // XCL version
    let xcl_source = r#"
        $i = 0
        $sum = 0
        while $i < 10000 {
            $sum = $sum + 1
            $i = $i + 1
        }
        $sum
    "#;
    let mut vm = xcl_plugins::new_vm();
    group.bench_function("xcl", |b| {
        b.iter(|| {
            let result = evaluate(black_box(xcl_source), black_box(&mut vm));
            vm.arena.reset();
            result
        })
    });

    // Luau version
    let lua = luau_vm();
    group.bench_function("luau", |b| {
        b.iter(|| {
            let result: i64 = lua
                .load("local i = 0; local sum = 0; while i < 10000 do sum = sum + 1; i = i + 1 end; return sum")
                .eval()
                .unwrap();
            black_box(result)
        })
    });

    group.finish();
}

/// Mixed arithmetic operations: add, sub, mul, div, mod
fn bench_mixed_arithmetic(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("mixed_arithmetic_20k");

    // XCL version
    let xcl_source = r#"
        $i = 0
        $a = 1
        $b = 2
        $c = 3
        while $i < 20000 {
            $a = $a + $b
            $b = $b - 1
            $c = $c * 2
            $a = $a / 2
            $b = $b % 3
            $i = $i + 1
        }
        $a
    "#;
    let mut vm = xcl_plugins::new_vm();
    group.bench_function("xcl", |b| {
        b.iter(|| {
            let result = evaluate(black_box(xcl_source), black_box(&mut vm));
            vm.arena.reset();
            result
        })
    });

    // Luau version
    let lua = luau_vm();
    group.bench_function("luau", |b| {
        b.iter(|| {
            let result: f64 = lua
                .load("local i = 0; local a = 1; local b = 2; local c = 3; while i < 20000 do a = a + b; b = b - 1; c = c * 2; a = a / 2; b = b % 3; i = i + 1 end; return a")
                .eval()
                .unwrap();
            black_box(result)
        })
    });

    group.finish();
}

/// Float arithmetic operations
fn bench_float_arithmetic(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("float_arithmetic_20k");

    // XCL version
    let xcl_source = r#"
        $i = 0
        $a = 1.5
        $b = 2.5
        $c = 3.14
        while $i < 20000 {
            $a = $a + $b
            $b = $b - 0.5
            $c = $c * 1.1
            $a = $a / 2.0
            $i = $i + 1
        }
        $a
    "#;
    let mut vm = xcl_plugins::new_vm();
    group.bench_function("xcl", |b| {
        b.iter(|| {
            let result = evaluate(black_box(xcl_source), black_box(&mut vm));
            vm.arena.reset();
            result
        })
    });

    // Luau version
    let lua = luau_vm();
    group.bench_function("luau", |b| {
        b.iter(|| {
            let result: f64 = lua
                .load("local i = 0; local a = 1.5; local b = 2.5; local c = 3.14; while i < 20000 do a = a + b; b = b - 0.5; c = c * 1.1; a = a / 2.0; i = i + 1 end; return a")
                .eval()
                .unwrap();
            black_box(result)
        })
    });

    group.finish();
}

/// Comparison and branching
fn bench_branching(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("branching_10k");

    // XCL version
    let xcl_source = r#"
        $i = 0
        $count = 0
        while $i < 10000 {
            if $i % 2 == 0 {
                $count = $count + 1
            }
            $i = $i + 1
        }
        $count
    "#;
    let mut vm = xcl_plugins::new_vm();
    group.bench_function("xcl", |b| {
        b.iter(|| {
            let result = evaluate(black_box(xcl_source), black_box(&mut vm));
            vm.arena.reset();
            result
        })
    });

    // Luau version
    let lua = luau_vm();
    group.bench_function("luau", |b| {
        b.iter(|| {
            let result: i64 = lua
                .load("local i = 0; local count = 0; while i < 10000 do if i % 2 == 0 then count = count + 1 end; i = i + 1 end; return count")
                .eval()
                .unwrap();
            black_box(result)
        })
    });

    group.finish();
}

/// Function call overhead
fn bench_function_calls(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("function_call_5k");

    // XCL version
    let xcl_source = "$add_one = ($x) => $x + 1; $i = 0; $result = 0; while $i < 5000 { $result = $add_one $result; $i = $i + 1 } $result";
    let mut vm = xcl_plugins::new_vm();
    group.bench_function("xcl", |b| {
        b.iter(|| {
            let result = evaluate(black_box(xcl_source), black_box(&mut vm));
            vm.arena.reset();
            result
        })
    });

    // Luau version
    let lua = luau_vm();
    group.bench_function("luau", |b| {
        b.iter(|| {
            let result: i64 = lua
                .load("local function add_one(x) return x + 1 end; local i = 0; local result = 0; while i < 5000 do result = add_one(result); i = i + 1 end; return result")
                .eval()
                .unwrap();
            black_box(result)
        })
    });

    group.finish();
}

/// Table/dict access in loops
fn bench_table_access(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("table_access_10k");

    // XCL version
    let xcl_source = r#"
        $m = { x: 1, y: 2, z: 3, w: 4, a: 5, b: 6, c: 7, d: 8 }
        $i = 0
        $sum = 0
        while $i < 10000 {
            $sum = $sum + $m.x + $m.y + $m.z + $m.w + $m.a + $m.b + $m.c + $m.d
            $i = $i + 1
        }
        $sum
    "#;
    let mut vm = xcl_plugins::new_vm();
    group.bench_function("xcl", |b| {
        b.iter(|| {
            let result = evaluate(black_box(xcl_source), black_box(&mut vm));
            vm.arena.reset();
            result
        })
    });

    // Luau version
    let lua = luau_vm();
    group.bench_function("luau", |b| {
        b.iter(|| {
            let result: i64 = lua
                .load("local m = {x=1,y=2,z=3,w=4,a=5,b=6,c=7,d=8}; local i = 0; local sum = 0; while i < 10000 do sum = sum + m.x + m.y + m.z + m.w + m.a + m.b + m.c + m.d; i = i + 1 end; return sum")
                .eval()
                .unwrap();
            black_box(result)
        })
    });

    group.finish();
}

/// Array iteration
fn bench_array_iteration(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("array_iteration_10k");

    // XCL version
    let xcl_source = r#"
        $arr = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
        $sum = 0
        $runs = 0
        while $runs < 1000 {
            for $item in $arr {
                $sum = $sum + $item
            }
            $runs = $runs + 1
        }
        $sum
    "#;
    let mut vm = xcl_plugins::new_vm();
    group.bench_function("xcl", |b| {
        b.iter(|| {
            let result = evaluate(black_box(xcl_source), black_box(&mut vm));
            vm.arena.reset();
            result
        })
    });

    // Luau version
    let lua = luau_vm();
    group.bench_function("luau", |b| {
        b.iter(|| {
            let result: i64 = lua
                .load("local arr = {1,2,3,4,5,6,7,8,9,10}; local sum = 0; local runs = 0; while runs < 1000 do for _,v in ipairs(arr) do sum = sum + v end; runs = runs + 1 end; return sum")
                .eval()
                .unwrap();
            black_box(result)
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_arithmetic_loop,
    bench_mixed_arithmetic,
    bench_float_arithmetic,
    bench_branching,
    bench_function_calls,
    bench_table_access,
    bench_array_iteration,
);
criterion_main!(benches);
