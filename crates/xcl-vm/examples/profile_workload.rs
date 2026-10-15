//! One-shot profiling workload for the XCL VM.
//!
//! This example runs a representative XCL workload multiple times
//! to produce enough samples for a clean flamegraph.
//! It is designed to be profiled with `samply` or `cargo-flamegraph`.

use xcl_vm::{VM, evaluate};

fn main() {
    // Mix of arithmetic, locals, branching, closures, and dict access —
    // the hottest paths in the VM dispatch loop.
    let source = r#"
        $i = 0
        $sum = 0
        while $i < 10000 {
            $sum = $sum + 1
            $i = $i + 1
        }
        $m = { x: 1, y: 2, z: 3 }
        $j = 0
        while $j < 5000 {
            $sum = $sum + $m.x + $m.y + $m.z
            $j = $j + 1
        }
        $sum
    "#;

    let mut vm = VM::new();
    vm.register_all();

    // Run the workload many times to accumulate profiling samples
    for _ in 0..100 {
        let _result = evaluate(source, &mut vm);
    }
}
