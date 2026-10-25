//! Temporary probe: how many heap allocations does each shape make per run?
//!
//! The VM's values live in a bump arena, so this counts *allocator* calls
//! (`Vec`, `HashMap`, `Box`), not arena bytes. Run with:
//!   cargo run --release -p xcl-vm --example alloc_probe

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static FREES: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        FREES.fetch_add(1, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

use std::time::Instant;
use xcl_toolchain::VM;
use xcl_toolchain::bytecode;
use xcl_toolchain::compiler::{lexer, parser};

fn compile(src: &str) -> bytecode::Bytecode {
    let tokens = lexer::lex(src, "<probe>");
    let ast = parser::parse(&tokens, src);
    bytecode::compile(&ast)
}

fn report(label: &str, src: &str, iters: usize) {
    let bc = compile(src);

    // Warm up: first run installs functions and grows Vec capacities.
    let mut vm = VM::new();
    std::hint::black_box(vm.run(&bc));

    ALLOCS.store(0, Ordering::Relaxed);
    FREES.store(0, Ordering::Relaxed);
    BYTES.store(0, Ordering::Relaxed);
    for _ in 0..iters {
        std::hint::black_box(vm.run(&bc));
    }
    let a = ALLOCS.load(Ordering::Relaxed) as f64 / iters as f64;
    let f = FREES.load(Ordering::Relaxed) as f64 / iters as f64;
    let b = BYTES.load(Ordering::Relaxed) as f64 / iters as f64;
    println!("{label:<28} allocs/run {a:>7.1}  frees/run {f:>7.1}  bytes/run {b:>9.0}");
}

fn time(label: &str, src: &str, iters: usize) {
    let bc = compile(src);
    let mut vm = VM::new();
    std::hint::black_box(vm.run(&bc));
    let start = Instant::now();
    for _ in 0..iters {
        std::hint::black_box(vm.run(&bc));
    }
    let ns = start.elapsed().as_nanos() as f64 / iters as f64;
    println!("{label:<28} time/run {ns:>9.0} ns");
}

/// Times the exact benchmark shape: a fresh `VM` per iteration, dropped after
/// the run, so setup + teardown are inside the measurement.
fn fresh(label: &str, src: &str, iters: usize) {
    let bc = compile(src);
    // Warm the arena pool so the first iteration is not the outlier.
    std::hint::black_box(VM::new().run(&bc));
    let start = Instant::now();
    for _ in 0..iters {
        let mut vm = VM::new();
        std::hint::black_box(vm.run(&bc));
    }
    let ns = start.elapsed().as_nanos() as f64 / iters as f64;
    println!("{label:<28} fresh {ns:>9.0} ns");
}

fn new_only(iters: usize) {
    let start = Instant::now();
    for _ in 0..iters {
        std::hint::black_box(VM::new());
    }
    let ns = start.elapsed().as_nanos() as f64 / iters as f64;
    println!("{:<28} new   {ns:>9.0} ns", "VM::new only");
}

/// Allocator calls for the whole `VM::new` + `run` + `drop` cycle.
///
/// The pool hands one set of buffers back and forth, so this is the steady
/// state a CLI sees: if it is non-zero, some per-run work still allocates.
fn fresh_allocs(label: &str, src: &str, iters: usize) {
    let bc = compile(src);
    std::hint::black_box(VM::new().run(&bc));
    ALLOCS.store(0, Ordering::Relaxed);
    FREES.store(0, Ordering::Relaxed);
    BYTES.store(0, Ordering::Relaxed);
    for _ in 0..iters {
        let mut vm = VM::new();
        std::hint::black_box(vm.run(&bc));
    }
    let a = ALLOCS.load(Ordering::Relaxed) as f64 / iters as f64;
    let f = FREES.load(Ordering::Relaxed) as f64 / iters as f64;
    let b = BYTES.load(Ordering::Relaxed) as f64 / iters as f64;
    println!("{label:<28} fresh allocs/run {a:>5.1}  frees {f:>5.1}  bytes {b:>8.0}");
}

/// Emulates criterion's `iter_batched(VM::new, |vm| vm.run(&bc), SmallInput)`:
/// a batch of VMs is constructed untimed and then run+dropped in the timed
/// region. This is the shape the benchmark actually measures.
fn batched(label: &str, src: &str, batch: usize, reps: usize) {
    let bc = compile(src);
    let mut best = f64::MAX;
    for _ in 0..reps {
        let mut vms: Vec<VM> = (0..batch).map(|_| VM::new()).collect();
        let start = Instant::now();
        for mut vm in vms.drain(..) {
            std::hint::black_box(vm.run(&bc));
        }
        let ns = start.elapsed().as_nanos() as f64 / batch as f64;
        best = best.min(ns);
    }
    println!("{label:<28} batched {best:>7.0} ns  (batch {batch})");
}

fn main() {
    let iters = 2_000;
    println!(
        "size_of::<VM>() = {} bytes",
        core::mem::size_of::<xcl_toolchain::VM>()
    );

    // Sanity: the counter must see std heap allocations.
    ALLOCS.store(0, Ordering::Relaxed);
    for _ in 0..1_000 {
        std::hint::black_box(String::from("start"));
    }
    println!(
        "String::from sanity          allocs/iter {:>7.1}",
        ALLOCS.load(Ordering::Relaxed) as f64 / 1_000.0
    );

    report("empty", "1", iters);
    report(
        "factorial_10 (calls)",
        r#"
fact($n) {
    if $n <= 1 { return 1 }
    $r = fact ($n - 1)
    return $n * $r
}
fact 10
"#,
        iters,
    );
    report(
        "for over 64-array",
        r#"
$a = [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63]
$s = 0
for $i in $a { $s = $s + $i }
$s
"#,
        iters,
    );
    report(
        "range value 0..4",
        r#"
$r = 0..4
$s = 0
for $i in $r { $s = $s + $i }
$s
"#,
        iters,
    );
    report(
        "map field read x1000",
        r#"
$m = {a: 1, b: 2}
$s = 0
$i = 0
while $i < 1000 {
    $s = $s + $m.a
    $i = $i + 1
}
$s
"#,
        iters,
    );
    report(
        "closure created x100",
        r#"
$i = 0
while $i < 100 {
    $f = |$x| $x + 1
    $f 1
    $i = $i + 1
}
$i
"#,
        iters,
    );
    report(
        "defer x100 (in helper)",
        r#"
h($n) {
    defer { $n = $n + 1 }
    return $n + 1
}
$i = 0
while $i < 100 { $i = h $i }
$i
"#,
        iters,
    );
    report(
        "string concat x200",
        r#"
$s = ""
$i = 0
while $i < 200 {
    $s = $s + "x"
    $i = $i + 1
}
$s
"#,
        iters,
    );

    report(
        "100 calls using $self",
        r#"
f($x) { return $self }
$i = 0
while $i < 100 { $i = $i + 1
    f $i
}
$i
"#,
        iters,
    );
    report(
        "100 calls using $0",
        r#"
g() { return $0 }
$i = 0
while $i < 100 { $i = $i + 1
    g
}
$i
"#,
        iters,
    );

    time(
        "time 100 closures",
        r#"
$i = 0
while $i < 100 {
    $f = |$x| $x + 1
    $f 1
    $i = $i + 1
}
$i
"#,
        iters,
    );
    time(
        "time 100 defers",
        r#"
h($n) {
    defer { $n = $n + 1 }
    return $n + 1
}
$i = 0
while $i < 100 { $i = h $i }
$i
"#,
        iters,
    );

    time(
        "time fib_20",
        r#"
fib($n) {
    if $n < 2 { return $n }
    return fib ($n - 1) + fib ($n - 2)
}
fib 20
"#,
        200,
    );
    time(
        "time mutual_2k",
        r#"
is_even($n) { if $n == 0 { return 1 } return is_odd ($n - 1) }
is_odd($n) { if $n == 0 { return 0 } return is_even ($n - 1) }
is_even 2000
"#,
        200,
    );

    new_only(iters);
    fresh_allocs("fresh allocs: empty", "1", iters);
    fresh_allocs(
        "fresh allocs: factorial_10",
        r#"
fact($n) {
    if $n <= 1 { return 1 }
    $r = fact ($n - 1)
    return $n * $r
}
fact 10
"#,
        iters,
    );
    fresh("fresh: empty", "1", iters);
    fresh(
        "fresh: factorial_10",
        r#"
fact($n) {
    if $n <= 1 { return 1 }
    $r = fact ($n - 1)
    return $n * $r
}
fact 10
"#,
        iters,
    );
    batched("batch: empty", "1", 2_360, 5);
    batched(
        "batch: factorial_10",
        r#"
fact($n) {
    if $n <= 1 { return 1 }
    $r = fact ($n - 1)
    return $n * $r
}
fact 10
"#,
        2_360,
        5,
    );
}
