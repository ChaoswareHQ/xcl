//! Integration benchmarks for xcl-std.
//!
//! These benchmarks exercise the full standard library including I/O,
//! Result/Error handling, and method dispatch.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use xcl_std::{evaluate_new, new_vm, evaluate};

/// Result construction and method chaining
fn bench_result_methods(c: &mut Criterion) {
    let source = r#"
        $i = 0
        $r = Ok 0
        while $i < 5000 {
            $r = Ok ($r.value + 1)
            $i = $i + 1
        }
        $r.value
    "#;

    c.bench_function("result_construction_5k", |b| {
        b.iter(|| {
            evaluate_new(black_box(source))
        })
    });
}

/// Error creation
fn bench_error_creation(c: &mut Criterion) {
    let source = r#"
        $i = 0
        $e = nil
        while $i < 5000 {
            $e = make_error "test" "message" $i
            $i = $i + 1
        }
        $e.code
    "#;

    c.bench_function("error_creation_5k", |b| {
        b.iter(|| {
            evaluate_new(black_box(source))
        })
    });
}

/// I/O write and read roundtrip
fn bench_io_roundtrip(c: &mut Criterion) {
    let path = std::env::temp_dir().join("xcl_bench_io.txt");

    c.bench_function("io_write_read_roundtrip", |b| {
        b.iter(|| {
            let mut vm = new_vm();
            let content = black_box("hello world from xcl benchmark\n");
            // Write
            evaluate(
                &format!(r#"io.write "{}" "{}""#, path.display(), content),
                black_box(&mut vm),
            );
            // Read
            evaluate(
                &format!(r#"io.read "{}""#, path.display()),
                black_box(&mut vm),
            );
        })
    });
    // Cleanup
    let _ = std::fs::remove_file(&path);
}

/// Domain expansion with io.print
fn bench_domain_expansion(c: &mut Criterion) {
    let source = r#"
        $i = 0
        while $i < 1000 {
            io {print: $i}
            $i = $i + 1
        }
        $i
    "#;

    c.bench_function("domain_expansion_1k", |b| {
        b.iter(|| {
            evaluate_new(black_box(source))
        })
    });
}

/// Full program: Fibonacci with function calls
fn bench_fibonacci(c: &mut Criterion) {
    let source = r#"
        fn fib($n) {
            if $n <= 1 {
                $n
            } else {
                fib($n - 1) + fib($n - 2)
            }
        }
        fib 20
    "#;

    c.bench_function("fibonacci_20", |b| {
        b.iter(|| {
            evaluate_new(black_box(source))
        })
    });
}

/// Array operations with push and iteration
fn bench_array_operations(c: &mut Criterion) {
    let source = r#"
        $arr = []
        $i = 0
        while $i < 1000 {
            $arr = $arr + [$i]
            $i = $i + 1
        }
        $sum = 0
        for $item in $arr {
            $sum = $sum + $item
        }
        $sum
    "#;

    c.bench_function("array_push_and_iter_1k", |b| {
        b.iter(|| {
            evaluate_new(black_box(source))
        })
    });
}

criterion_group!(
    benches,
    bench_result_methods,
    bench_error_creation,
    bench_io_roundtrip,
    bench_domain_expansion,
    bench_fibonacci,
    bench_array_operations,
);
criterion_main!(benches);
