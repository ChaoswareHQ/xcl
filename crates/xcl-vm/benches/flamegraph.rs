//! Focused benchmark for flamegraph generation.
//!
//! This benchmark runs a single representative workload that exercises
//! the most common opcodes: arithmetic, local variable access, and branching.
//! It is designed to produce a clean flamegraph showing the VM dispatch loop.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use xcl_vm::{VM, evaluate};

/// A single benchmark that mixes arithmetic, locals, and branching —
/// the hottest paths in the VM dispatch loop.
fn bench_flamegraph_workload(c: &mut Criterion) {
    // This workload exercises: OP_PUSH_INT, OP_LOAD_LOCAL, OP_STORE_LOCAL,
    // OP_ADD, OP_LT, OP_JUMP_IF_FALSE, OP_JUMP — the top 7 opcodes by frequency.
    let source = r#"
        $i = 0
        $sum = 0
        while $i < 10000 {
            $sum = $sum + 1
            $i = $i + 1
        }
        $sum
    "#;

    let mut vm = VM::new();
    vm.register_all();
    c.bench_function("vm_dispatch_loop", |b| {
        b.iter(|| evaluate(black_box(source), black_box(&mut vm)))
    });
}

criterion_group!(benches, bench_flamegraph_workload);
criterion_main!(benches);
