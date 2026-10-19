//! Microbenchmarks for the XCL VM dispatch loop.
//!
//! Each benchmark exercises a different set of opcodes to measure
//! the hot paths in the interpreter.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use xcl_vm::evaluate;

/// Tight arithmetic loop: exercises OP_ADD, OP_LOAD_LOCAL, OP_JUMP_IF_FALSE, OP_STORE_LOCAL
fn bench_arithmetic_loop(c: &mut Criterion) {
    let source = r#"
        $i = 0
        $sum = 0
        while $i < 10000 {
            $sum = $sum + 1
            $i = $i + 1
        }
        $sum
    "#;

    let mut vm = xcl_plugins::new_vm();
    c.bench_function("arithmetic_loop_10k", |b| {
        b.iter(|| {
            let result = evaluate(black_box(source), black_box(&mut vm));
            #[cfg(feature = "opcode-stats")]
            {
                vm.arena.reset();
            }
            result
        })
    });
    #[cfg(feature = "opcode-stats")]
    vm.print_opcode_histogram(15);
}

/// Repeated closure calls: exercises OP_CALL, OP_LOAD_LAMBDA, frame management
fn bench_function_calls(c: &mut Criterion) {
    let source = "$add_one = ($x) => $x + 1; $i = 0; $result = 0; while $i < 5000 { $result = $add_one $result; $i = $i + 1 } $result";

    let mut vm = xcl_plugins::new_vm();
    c.bench_function("closure_call_5k", |b| {
        b.iter(|| {
            let result = evaluate(black_box(source), black_box(&mut vm));
            #[cfg(feature = "opcode-stats")]
            {
                vm.arena.reset();
            }
            result
        })
    });
    #[cfg(feature = "opcode-stats")]
    vm.print_opcode_histogram(15);
}

/// Closure calls with captured variables: exercises closures and frame management
fn bench_closure_calls(c: &mut Criterion) {
    let source = "$multiplier = 2; $double = ($x) => $x * $multiplier; $i = 0; $result = 0; while $i < 5000 { $result = $double $result; $i = $i + 1 } $result";

    let mut vm = xcl_plugins::new_vm();
    c.bench_function("closure_capture_5k", |b| {
        b.iter(|| {
            let result = evaluate(black_box(source), black_box(&mut vm));
            #[cfg(feature = "opcode-stats")]
            {
                vm.arena.reset();
            }
            result
        })
    });
    #[cfg(feature = "opcode-stats")]
    vm.print_opcode_histogram(15);
}

/// Array iteration: exercises OP_FOR_ITER_START, OP_FOR_ITER_NEXT
fn bench_array_iteration(c: &mut Criterion) {
    let source = r#"
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
    c.bench_function("array_iteration_10k_items", |b| {
        b.iter(|| {
            let result = evaluate(black_box(source), black_box(&mut vm));
            #[cfg(feature = "opcode-stats")]
            {
                vm.arena.reset();
            }
            result
        })
    });
    #[cfg(feature = "opcode-stats")]
    vm.print_opcode_histogram(15);
}

/// Range-based loop: exercises OP_FOR_RANGE_START, OP_FOR_RANGE_NEXT
fn bench_range_loop(c: &mut Criterion) {
    let source = r#"
        $sum = 0
        for $i in 0..10000 {
            $sum = $sum + $i
        }
        $sum
    "#;

    let mut vm = xcl_plugins::new_vm();
    c.bench_function("range_loop_10k", |b| {
        b.iter(|| {
            let result = evaluate(black_box(source), black_box(&mut vm));
            #[cfg(feature = "opcode-stats")]
            {
                vm.arena.reset();
            }
            result
        })
    });
    #[cfg(feature = "opcode-stats")]
    vm.print_opcode_histogram(15);
}

/// Map/dict access: exercises OP_GET_FIELD, OP_MAKE_DICT
fn bench_dict_access(c: &mut Criterion) {
    let source = r#"
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
    c.bench_function("dict_access_10k", |b| {
        b.iter(|| {
            let result = evaluate(black_box(source), black_box(&mut vm));
            #[cfg(feature = "opcode-stats")]
            {
                vm.arena.reset();
            }
            result
        })
    });
    #[cfg(feature = "opcode-stats")]
    vm.print_opcode_histogram(15);
}

/// Comparison and branching: exercises OP_LT, OP_JUMP_IF_FALSE, OP_EQ, OP_NEQ
fn bench_branching(c: &mut Criterion) {
    let source = r#"
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
    c.bench_function("branching_10k", |b| {
        b.iter(|| {
            let result = evaluate(black_box(source), black_box(&mut vm));
            #[cfg(feature = "opcode-stats")]
            {
                vm.arena.reset();
            }
            result
        })
    });
    #[cfg(feature = "opcode-stats")]
    vm.print_opcode_histogram(15);
}

/// Nested closure calls: exercises deep frame management
fn bench_nested_calls(c: &mut Criterion) {
    let source = "$level3 = ($x) => $x + 1; $level2 = ($x) => $level3 $x; $level1 = ($x) => $level2 $x; $i = 0; $result = 0; while $i < 2000 { $result = $level1 $result; $i = $i + 1 } $result";

    let mut vm = xcl_plugins::new_vm();
    c.bench_function("nested_closure_2k", |b| {
        b.iter(|| {
            let result = evaluate(black_box(source), black_box(&mut vm));
            #[cfg(feature = "opcode-stats")]
            {
                vm.arena.reset();
            }
            result
        })
    });
    #[cfg(feature = "opcode-stats")]
    vm.print_opcode_histogram(15);
}

/// String concatenation: exercises heap allocation via arena
fn bench_string_concat(c: &mut Criterion) {
    let source = r#"
        $i = 0
        $s = ""
        while $i < 1000 {
            $s = $s + "x"
            $i = $i + 1
        }
        len $s
    "#;

    let mut vm = xcl_plugins::new_vm();
    c.bench_function("string_concat_1k", |b| {
        b.iter(|| {
            let result = evaluate(black_box(source), black_box(&mut vm));
            #[cfg(feature = "opcode-stats")]
            {
                vm.arena.reset();
            }
            result
        })
    });
    #[cfg(feature = "opcode-stats")]
    vm.print_opcode_histogram(15);
}

/// Fast-path integer arithmetic: exercises the int+int fast path in OP_ADD, OP_SUB, OP_MUL
fn bench_fastpath_int_arithmetic(c: &mut Criterion) {
    let source = r#"
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
    c.bench_function("fastpath_int_arithmetic_20k", |b| {
        b.iter(|| {
            let result = evaluate(black_box(source), black_box(&mut vm));
            #[cfg(feature = "opcode-stats")]
            {
                vm.arena.reset();
            }
            result
        })
    });
    #[cfg(feature = "opcode-stats")]
    vm.print_opcode_histogram(15);
}

/// Fast-path float arithmetic: exercises the float+float fast path
fn bench_fastpath_float_arithmetic(c: &mut Criterion) {
    let source = r#"
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
    c.bench_function("fastpath_float_arithmetic_20k", |b| {
        b.iter(|| {
            let result = evaluate(black_box(source), black_box(&mut vm));
            #[cfg(feature = "opcode-stats")]
            {
                vm.arena.reset();
            }
            result
        })
    });
    #[cfg(feature = "opcode-stats")]
    vm.print_opcode_histogram(15);
}

/// Fast-path comparison: exercises int/int and float/float comparison fast paths
fn bench_fastpath_comparison(c: &mut Criterion) {
    let source = r#"
        $i = 0
        $count = 0
        while $i < 20000 {
            if $i < 10000 {
                $count = $count + 1
            }
            if $i > 5000 {
                $count = $count + 1
            }
            if $i <= 15000 {
                $count = $count + 1
            }
            if $i >= 100 {
                $count = $count + 1
            }
            if $i == 10000 {
                $count = $count + 1
            }
            if $i != 0 {
                $count = $count + 1
            }
            $i = $i + 1
        }
        $count
    "#;

    let mut vm = xcl_plugins::new_vm();
    c.bench_function("fastpath_comparison_20k", |b| {
        b.iter(|| {
            let result = evaluate(black_box(source), black_box(&mut vm));
            #[cfg(feature = "opcode-stats")]
            {
                vm.arena.reset();
            }
            result
        })
    });
    #[cfg(feature = "opcode-stats")]
    vm.print_opcode_histogram(15);
}

criterion_group!(
    benches,
    bench_arithmetic_loop,
    bench_function_calls,
    bench_closure_calls,
    bench_array_iteration,
    bench_range_loop,
    bench_dict_access,
    bench_branching,
    bench_nested_calls,
    bench_string_concat,
    bench_fastpath_int_arithmetic,
    bench_fastpath_float_arithmetic,
    bench_fastpath_comparison,
);
criterion_main!(benches);
