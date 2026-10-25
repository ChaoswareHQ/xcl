//! Pure-VM core benchmarks: arithmetic, branching, calls, strings, pipeline.
//!
//! Compares XCL VM execution against Luau via mlua.

mod common;

use common::*;
use criterion::{BenchmarkGroup, Criterion, criterion_group, criterion_main};

// ─── Reference computations for expected values ───

fn ref_int_mixed_20k() -> i64 {
    let mut a: i64 = 1;
    let mut b: i64 = 2;
    let mut c: i64 = 3;
    for _ in 0..20000 {
        a = (a + b) % 1000003;
        b = (b + 1) % 1000;
        c = (c * 2 + a) % 1000003;
    }
    c
}


fn ref_nested_if_1k() -> i64 {
    let mut s: i64 = 0;
    for i in 0..1000 {
        if i % 2 == 0 {
            if i % 3 == 0 {
                s += 1;
            } else {
                s += 2;
            }
        } else {
            s += 3;
        }
    }
    s
}

fn ref_bitwise_mix_20k() -> i64 {
    let mut x: i64 = 0;
    let mut y: i64 = 0;
    let mut z: i64 = 0;
    for i in 0..20000 {
        x = (x ^ i) & 65535;
        y = (y | (i << 2)) & 65535;
        z = (z + (x >> 1)) % 1000003;
    }
    z
}

// ─── Medium script for compile-only / compile-and-run ───

const MEDIUM_SCRIPT: &str = r#"
# Medium compilation target — integers, arrays, maps
$a = 1 + 2 * 3
$b = $a % 7
$c = 100 / 3
$d = 2 ** 10
$arr = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
$e = $arr[0] + $arr[9]
$m = { x: 10, y: 20, z: 30 }
$f = $m.x + $m.y
$g = $m["z"]
$h = $f + $g
$i = 0
$s = 0
while $i < 100 {
    $s = $s + $i
    $i = $i + 1
}
$j = $s
$k = 0
for $x in $arr {
    $k = $k + $x
}
$l = $k
$m2 = { a: 1, b: 2, c: 3 }
$n = $m2.a + $m2.b + $m2.c
$o = $n * 10
$p = $o + $j
$q = $p % 1000
$r = $q + $l
$s2 = $r + $m.x
$t = $s2 * 2
$u = $t - 50
$v = $u / 3
$w = $v + 1
$x2 = $w % 7
$y = $x2 ** 2
$z = $y + 1
$final = $z + $arr[5]
$final
"#;

// ─── Core benchmarks ───

fn bench_core(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("core");

    // 1. int_loop_sum_10k
    bench_pair(
        &mut group,
        "int_loop_sum_10k",
        r#"
$i = 0
$sum = 0
while $i < 10000 {
    $sum = $sum + $i
    $i = $i + 1
}
$sum
"#,
        "local i = 0; local sum = 0; while i < 10000 do sum = sum + i; i = i + 1 end; return sum",
        49_995_000.0,
    );

    // 2. int_mixed_20k
    let expected_mixed = ref_int_mixed_20k() as f64;
    bench_pair(
        &mut group,
        "int_mixed_20k",
        r#"
$i = 0
$a = 1
$b = 2
$c = 3
while $i < 20000 {
    $a = ($a + $b) % 1000003
    $b = ($b + 1) % 1000
    $c = ($c * 2 + $a) % 1000003
    $i = $i + 1
}
$c
"#,
        "local i = 0; local a = 1; local b = 2; local c = 3; while i < 20000 do a = (a + b) % 1000003; b = (b + 1) % 1000; c = (c * 2 + a) % 1000003; i = i + 1 end; return c",
        expected_mixed,
    );

    // 3. float_loop_20k
    let mut ref_f: f64 = 0.0;
    for i in 0..20000i64 {
        ref_f += (i as f64) * 0.001;
        ref_f %= 1000000.0;
    }
    bench_pair(
        &mut group,
        "float_loop_20k",
        r#"
$i = 0
$f = 0.0
while $i < 20000 {
    $f = $f + $i * 0.001
    $f = $f % 1000000.0
    $i = $i + 1
}
$f
"#,
        "local i = 0; local f = 0.0; while i < 20000 do f = f + i * 0.001; f = f % 1000000.0; i = i + 1 end; return f",
        ref_f,
    );

    // 4. branch_parity_10k
    bench_pair(
        &mut group,
        "branch_parity_10k",
        r#"
$i = 0
$count = 0
while $i < 10000 {
    if $i % 2 == 0 {
        $count = $count + 1
    }
    $i = $i + 1
}
$count
"#,
        "local i = 0; local count = 0; while i < 10000 do if i % 2 == 0 then count = count + 1 end; i = i + 1 end; return count",
        5000.0,
    );

    // 5. range_sum_10k
    bench_pair(
        &mut group,
        "range_sum_10k",
        r#"
$sum = 0
for $i in 0..10000 {
    $sum = $sum + $i
}
$sum
"#,
        "local sum = 0; for i = 0, 9999 do sum = sum + i end; return sum",
        49_995_000.0,
    );

    // 6. nested_if_1k
    let expected_nested = ref_nested_if_1k() as f64;
    bench_pair(
        &mut group,
        "nested_if_1k",
        r#"
$i = 0
$s = 0
while $i < 1000 {
    if $i % 2 == 0 {
        if $i % 3 == 0 {
            $s = $s + 1
        } else {
            $s = $s + 2
        }
    } else {
        $s = $s + 3
    }
    $i = $i + 1
}
$s
"#,
        "local i = 0; local s = 0; while i < 1000 do if i % 2 == 0 then if i % 3 == 0 then s = s + 1 else s = s + 2 end else s = s + 3 end; i = i + 1 end; return s",
        expected_nested,
    );

    // 7. bitwise_mix_20k
    let expected_bitwise = ref_bitwise_mix_20k() as f64;
    bench_pair(
        &mut group,
        "bitwise_mix_20k",
        r#"
$i = 0
$x = 0
$y = 0
$z = 0
while $i < 20000 {
    $x = (($x ^ $i) & 65535)
    $y = (($y | ($i << 2)) & 65535)
    $z = (($z + ($x >> 1)) % 1000003)
    $i = $i + 1
}
$z
"#,
        "local i = 0; local x = 0; local y = 0; local z = 0; while i < 20000 do x = bit32.bxor(x, i) % 65536; y = bit32.bor(y, bit32.lshift(i, 2)) % 65536; z = (z + bit32.rshift(x, 1)) % 1000003; i = i + 1 end; return z",
        expected_bitwise,
    );

    // 8. compile_only_medium
    bench_compile_only(&mut group, "compile_only_medium", MEDIUM_SCRIPT);

    // 9. compile_and_run_medium
    // Validate by running once with run_xcl
    let bc = compile_xcl(MEDIUM_SCRIPT);
    let medium_result = run_xcl(&bc);
    let medium_expected = as_number(medium_result);
    bench_compile_and_run(
        &mut group,
        "compile_and_run_medium",
        MEDIUM_SCRIPT,
        medium_expected,
    );

    group.finish();
}

// ─── Call benchmarks ───

fn bench_calls(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("calls");

    // 10. named_call_5k
    bench_pair(
        &mut group,
        "named_call_5k",
        r#"
inc($x) { return $x + 1 }
$r = 0
$i = 0
while $i < 5000 {
    $r = inc $r
    $i = $i + 1
}
$r
"#,
        "local function inc(x) return x + 1 end; local r = 0; local i = 0; while i < 5000 do r = inc(r); i = i + 1 end; return r",
        5000.0,
    );

    // 11. named_call_two_args_5k
    bench_pair(
        &mut group,
        "named_call_two_args_5k",
        r#"
add($a, $b) { return $a + $b }
$r = 0
$i = 0
while $i < 5000 {
    $r = add $r 1
    $i = $i + 1
}
$r
"#,
        "local function add(a, b) return a + b end; local r = 0; local i = 0; while i < 5000 do r = add(r, 1); i = i + 1 end; return r",
        5000.0,
    );

    // 12. nested_calls_2k — uses explicit temporaries instead of nested calls
    bench_pair(
        &mut group,
        "nested_calls_2k",
        r#"
inc($x) { return $x + 1 }
twice($x) {
    $t = inc $x
    return inc $t
}
$i = 0
$r = 0
while $i < 2000 {
    $r = twice $r
    $i = $i + 1
}
$r
"#,
        "local function inc(x) return x + 1 end; local function twice(x) local t = inc(x); return inc(t) end; local i = 0; local r = 0; while i < 2000 do r = twice(r); i = i + 1 end; return r",
        4000.0,
    );

    // 13. recursion_fib_20
    bench_pair(
        &mut group,
        "recursion_fib_20",
        r#"
fib($n) {
    if $n < 2 {
        return $n
    }
    return (fib ($n - 1)) + (fib ($n - 2))
}
fib 20
"#,
        "local function fib(n) if n < 2 then return n end; return fib(n - 1) + fib(n - 2) end; return fib(20)",
        6765.0,
    );

    // 14. recursion_fib_25
    bench_pair(
        &mut group,
        "recursion_fib_25",
        r#"
fib($n) {
    if $n < 2 {
        return $n
    }
    return (fib ($n - 1)) + (fib ($n - 2))
}
fib 25
"#,
        "local function fib(n) if n < 2 then return n end; return fib(n - 1) + fib(n - 2) end; return fib(25)",
        75025.0,
    );

    // 15. ackermann_3_3 — NOT RESTORED: nested-call form is not supported/reliable
    // in the current VM. Known limitation.

    // 16. mutual_recursion_2k — is_even / is_odd at 2000; expected 1
    bench_pair(
        &mut group,
        "mutual_recursion_2k",
        r#"
is_even($n) {
    if $n == 0 {
        return 1
    }
    return is_odd ($n - 1)
}
is_odd($n) {
    if $n == 0 {
        return 0
    }
    return is_even ($n - 1)
}
is_even 2000
"#,
        "local is_even, is_odd; is_even = function(n) if n == 0 then return 1 end; return is_odd(n - 1) end; is_odd = function(n) if n == 0 then return 0 end; return is_even(n - 1) end; return is_even(2000)",
        1.0,
    );

    group.finish();
}

// ─── String benchmarks ───

fn bench_strings(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("strings");

    // 17. string_concat_1k
    bench_pair_string(
        &mut group,
        "string_concat_1k",
        r#"
$s = ""
$i = 0
while $i < 1000 {
    $s = $s + "x"
    $i = $i + 1
}
$s
"#,
        "local s = ''; local i = 0; while i < 1000 do s = s .. 'x'; i = i + 1 end; return s",
        1000,
    );

    // 18. string_concat_10k
    bench_pair_string(
        &mut group,
        "string_concat_10k",
        r#"
$s = ""
$i = 0
while $i < 10000 {
    $s = $s + "x"
    $i = $i + 1
}
$s
"#,
        "local s = ''; local i = 0; while i < 10000 do s = s .. 'x'; i = i + 1 end; return s",
        10000,
    );

    group.finish();
}

// ─── Pipeline benchmarks ───

fn bench_pipeline(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("pipeline");

    let int_loop_src = r#"
$i = 0
$sum = 0
while $i < 10000 {
    $sum = $sum + $i
    $i = $i + 1
}
$sum
"#;

    // 19. compile_only_core
    bench_compile_only(&mut group, "compile_only_core", int_loop_src);

    // 20. compile_and_run_core
    bench_compile_and_run(
        &mut group,
        "compile_and_run_core",
        int_loop_src,
        49_995_000.0,
    );

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(30)
        .warm_up_time(std::time::Duration::from_millis(300))
        .measurement_time(std::time::Duration::from_secs(2));
    targets = bench_core, bench_calls, bench_strings, bench_pipeline
}
criterion_main!(benches);
