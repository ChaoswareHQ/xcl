//! Interpreter overhead benchmarks for xcl-rt.
//!
//! Measures the core interpreter costs that the performance analysis identified
//! as bottlenecks: dispatch overhead, value operations, and opcode frequency.
//!
//! Run with:
//! ```sh
//! cargo bench -p xcl-benchmark --bench interpreter
//! ```

use criterion::{BenchmarkGroup, BatchSize, Criterion, measurement::Measurement, criterion_group, criterion_main};
use std::collections::HashMap;
use std::time::Duration;
use xcl_rt::{Bytecode, Value, VM, new_vm};

// ─── Micro-benchmarks: individual operation costs ───

/// Measure the cost of creating and destroying a VM instance.
fn bench_vm_creation<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    group.bench_function("vm_creation", |b| {
        b.iter(|| {
            let _vm = VM::new();
        })
    });
}

/// Measure the cost of creating a fully-registered VM (with stdlib).
fn bench_vm_with_stdlib<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    group.bench_function("vm_with_stdlib", |b| {
        b.iter(|| {
            let _vm = new_vm();
        })
    });
}

/// Measure the cost of running an empty bytecode (just dispatch loop overhead).
fn bench_empty_dispatch<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    let bc = Bytecode::default();
    group.bench_function("empty_dispatch", |b| {
        b.iter(|| {
            let mut vm = new_vm();
            let _ = vm.run(&bc);
        })
    });
}

/// Measure NaN-boxing overhead: create, check tag, extract, rebox.
fn bench_nanboxing<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    group.bench_function("nanbox_int_roundtrip", |b| {
        b.iter(|| {
            let v = Value::from_i48(12345i64);
            let _tag = v.is_int();
            let _val = v.as_i48();
            let _reboxed = Value::from_i48(_val);
        })
    });
}

/// Measure HashMap field lookup cost (the main bottleneck for map_read).
fn bench_hashmap_lookup<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    let mut map: HashMap<String, u32> = HashMap::new();
    for i in 0..8 {
        let key = format!("field_{}", i);
        map.insert(key, i);
    }
    
    group.bench_function("hashmap_field_lookup", |b| {
        b.iter(|| {
            let mut sum = 0u32;
            for i in 0..8 {
                if let Some(&val) = map.get(&format!("field_{}", i)) {
                    sum += val;
                }
            }
            sum
        })
    });
}

/// Measure FNV-1a hash cost (used for field name lookups).
fn bench_fnv_hash<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
    
    fn fnv1a(bytes: &[u8]) -> u64 {
        let mut h = FNV_OFFSET;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01B3);
        }
        h
    }
    
    let names = vec![
        "a", "b", "c", "d", "e", "f", "g", "h",
        "field_name", "another_field", "very_long_field_name",
    ];
    
    group.bench_function("fnv1a_hash", |b| {
        b.iter(|| {
            let mut sum = 0u64;
            for name in &names {
                sum = sum.wrapping_add(fnv1a(name.as_bytes()));
            }
            sum
        })
    });
}

// ─── Bytecode-level benchmarks ───

/// Compile and run the crosslang benchmark scripts, measuring interpreter overhead.
fn bench_crosslang_interpreter<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    let benchmark_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/crosslang/xcl");
    let workloads = [
        "int_loop", "lcg", "bitwise", "fib", "hanoi",
        "map_read", "array_sum", "entity_oop", "native_call",
    ];
    
    for name in &workloads {
        let bc_path = format!("{}/{}.xclb", benchmark_dir, name);
        if let Ok(bc) = Bytecode::from_file(std::path::Path::new(&bc_path)) {
            group.bench_function(format!("interpreter_{}", name), |b| {
                b.iter_batched(
                    || (bc.clone(), VM::new()),
                    |(bc, mut vm)| vm.run(&bc),
                    BatchSize::SmallInput,
                )
            });
        }
    }
}

// ─── Dispatch overhead measurement ───

/// Measure the per-instruction dispatch overhead by running a known bytecode
/// and dividing total time by instruction count.
fn bench_dispatch_overhead<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    // A simple loop generates predictable bytecode
    let benchmark_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/crosslang/xcl");
    
    for name in &["int_loop", "lcg", "bitwise"] {
        let bc_path = format!("{}/{}.xclb", benchmark_dir, name);
        if let Ok(bc) = Bytecode::from_file(std::path::Path::new(&bc_path)) {
            let instr_count = bc.instructions.len();
            
            group.bench_function(
                format!("dispatch_overhead_{}_({}instr)", name, instr_count),
                |b| {
                    b.iter_batched(
                        || (bc.clone(), VM::new()),
                        |(bc, mut vm)| vm.run(&bc),
                        BatchSize::SmallInput,
                    )
                },
            );
            
            // Print per-instruction estimate
            println!(
                "[info] {}: {} instructions in bytecode",
                name, instr_count
            );
        }
    }
}

// ─── Arena allocation benchmarks ───

/// Measure value creation cost (strings allocated on arena).
fn bench_value_creation<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    group.bench_function("value_string_creation", |b| {
        b.iter(|| {
            let mut sum = 0u64;
            for i in 0..100 {
                let v = Value::from_str_boxed(format!("string_{}", i));
                if let Some(s) = v.as_str() {
                    sum = sum.wrapping_add(s.len() as u64);
                }
            }
            sum
        })
    });
}

// ─── Main ───

fn bench_interpreter(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("interpreter");
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(100);
    
    // Micro-benchmarks
    bench_vm_creation(&mut group);
    bench_vm_with_stdlib(&mut group);
    bench_empty_dispatch(&mut group);
    bench_nanboxing(&mut group);
    bench_hashmap_lookup(&mut group);
    bench_fnv_hash(&mut group);
    
    // Value creation benchmarks
    bench_value_creation(&mut group);
    
    // Dispatch overhead
    bench_dispatch_overhead(&mut group);
    
    // Full crosslang benchmarks
    bench_crosslang_interpreter(&mut group);
    
    group.finish();
}

criterion_group!(benches, bench_interpreter);
criterion_main!(benches);
