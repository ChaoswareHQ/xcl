//! Pure-VM data structure benchmarks: arrays, maps, mixed.
//!
//! Compares XCL VM execution against Luau via mlua.

mod common;

use common::*;
use criterion::{BenchmarkGroup, Criterion, criterion_group, criterion_main};

// ─── Array benchmarks ───

fn bench_arrays(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("arrays");

    // 1. array_for_sum_100x100
    // Array 0..99, for $x in $a repeated 100 times
    // Sum of 0..99 = 4950, times 100 = 495000
    bench_pair(
        &mut group,
        "array_for_sum_100x100",
        r#"
$a = []
$i = 0
while $i < 100 {
    $a = $a + [$i]
    $i = $i + 1
}
$s = 0
$r = 0
while $r < 100 {
    for $x in $a {
        $s = $s + $x
    }
    $r = $r + 1
}
$s
"#,
        "local a = {}; for i = 0, 99 do table.insert(a, i) end; local s = 0; local r = 0; while r < 100 do for _, v in ipairs(a) do s = s + v end; r = r + 1 end; return s",
        495_000.0,
    );

    // 2. array_index_sum_10k
    // Array 0..9, while 10000, $s = $s + $a[$i % 10]
    // Each iteration sums 0+1+...+9 = 45, but it's $a[$i % 10] which cycles through 0..9
    // 10000 iterations, each adds one element. Sum of 0..9 repeated 1000 times = 45 * 1000 = 45000
    bench_pair(
        &mut group,
        "array_index_sum_10k",
        r#"
$a = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
$i = 0
$s = 0
while $i < 10000 {
    $s = $s + $a[$i % 10]
    $i = $i + 1
}
$s
"#,
        "local a = {0,1,2,3,4,5,6,7,8,9}; local i = 0; local s = 0; while i < 10000 do s = s + a[(i % 10) + 1]; i = i + 1 end; return s",
        45_000.0,
    );

    // 3. array_push_10k
    // Push $i 10000 times, return $a[9999]
    bench_pair(
        &mut group,
        "array_push_10k",
        r#"
$a = []
$i = 0
while $i < 10000 {
    $a = $a + [$i]
    $i = $i + 1
}
$a[9999]
"#,
        "local a = {}; local i = 0; while i < 10000 do table.insert(a, i); i = i + 1 end; return a[10000]",
        9999.0,
    );

    // 4. array_concat_double_10
    // $a = [1], 10 times $a = $a + $a; return $a[1023]
    // After 10 doublings: 2^10 = 1024 elements, all 1. $a[1023] = 1
    bench_pair(
        &mut group,
        "array_concat_double_10",
        r#"
$a = [1]
$i = 0
while $i < 10 {
    $a = $a + $a
    $i = $i + 1
}
$a[1023]
"#,
        "local a = {1}; local i = 0; while i < 10 do local n = #a; for j = 1, n do table.insert(a, a[j]) end; i = i + 1 end; return a[1024]",
        1.0,
    );

    // 5. nested_array_sum
    // 10x10 literal matrix where row r is [10r, 10r+1, ...]
    // Sum of all elements: sum over r=0..9 of sum over c=0..9 of (10r+c)
    // = sum_r(10r*10 + 45) = 100*sum_r(r) + 450 = 100*45 + 450 = 4500 + 450 = 4950
    bench_pair(
        &mut group,
        "nested_array_sum",
        r#"
$m = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
    [10, 11, 12, 13, 14, 15, 16, 17, 18, 19],
    [20, 21, 22, 23, 24, 25, 26, 27, 28, 29],
    [30, 31, 32, 33, 34, 35, 36, 37, 38, 39],
    [40, 41, 42, 43, 44, 45, 46, 47, 48, 49],
    [50, 51, 52, 53, 54, 55, 56, 57, 58, 59],
    [60, 61, 62, 63, 64, 65, 66, 67, 68, 69],
    [70, 71, 72, 73, 74, 75, 76, 77, 78, 79],
    [80, 81, 82, 83, 84, 85, 86, 87, 88, 89],
    [90, 91, 92, 93, 94, 95, 96, 97, 98, 99]
]
$s = 0
$i = 0
while $i < 10 {
    $j = 0
    while $j < 10 {
        $s = $s + $m[$i][$j]
        $j = $j + 1
    }
    $i = $i + 1
}
$s
"#,
        "local m = {{0,1,2,3,4,5,6,7,8,9},{10,11,12,13,14,15,16,17,18,19},{20,21,22,23,24,25,26,27,28,29},{30,31,32,33,34,35,36,37,38,39},{40,41,42,43,44,45,46,47,48,49},{50,51,52,53,54,55,56,57,58,59},{60,61,62,63,64,65,66,67,68,69},{70,71,72,73,74,75,76,77,78,79},{80,81,82,83,84,85,86,87,88,89},{90,91,92,93,94,95,96,97,98,99}}; local s = 0; local i = 1; while i <= 10 do local j = 1; while j <= 10 do s = s + m[i][j]; j = j + 1 end; i = i + 1 end; return s",
        4950.0,
    );

    // 6. array_of_maps_1k
    // Array of 100 maps {v: i}; loop 10 times summing $a[$j].v
    // Sum of v=0..99 = 4950, times 10 = 49500
    bench_pair(
        &mut group,
        "array_of_maps_1k",
        r#"
$a = []
$i = 0
while $i < 100 {
    $a = $a + [{ v: $i }]
    $i = $i + 1
}
$s = 0
$r = 0
while $r < 10 {
    $j = 0
    while $j < 100 {
        $s = $s + $a[$j].v
        $j = $j + 1
    }
    $r = $r + 1
}
$s
"#,
        "local a = {}; local i = 0; while i < 100 do table.insert(a, {v = i}); i = i + 1 end; local s = 0; local r = 0; while r < 10 do local j = 1; while j <= 100 do s = s + a[j].v; j = j + 1 end; r = r + 1 end; return s",
        49_500.0,
    );

    group.finish();
}

// ─── Map benchmarks ───

fn bench_maps(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("maps");

    // 7. map_field_read_10k
    // Map {a:1,...,h:8}; loop 10000 summing all fields
    // Sum per iteration: 1+2+3+4+5+6+7+8 = 36
    // Total: 36 * 10000 = 360000
    bench_pair(
        &mut group,
        "map_field_read_10k",
        r#"
$m = { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8 }
$i = 0
$s = 0
while $i < 10000 {
    $s = $s + $m.a
    $s = $s + $m.b
    $s = $s + $m.c
    $s = $s + $m.d
    $s = $s + $m.e
    $s = $s + $m.f
    $s = $s + $m.g
    $s = $s + $m.h
    $i = $i + 1
}
$s
"#,
        "local m = {a=1,b=2,c=3,d=4,e=5,f=6,g=7,h=8}; local i = 0; local s = 0; while i < 10000 do s = s + m.a + m.b + m.c + m.d + m.e + m.f + m.g + m.h; i = i + 1 end; return s",
        360_000.0,
    );

    // 8. map_index_read_10k — bracket-notation reads inside while loop
    // Map {a:1,...,h:8}; loop 10000 summing all via $m["key"]
    // Sum per iteration: 1+2+3+4+5+6+7+8 = 36
    // Total: 36 * 10000 = 360000
    bench_pair(
        &mut group,
        "map_index_read_10k",
        r#"
$m = { a:1, b:2, c:3, d:4, e:5, f:6, g:7, h:8 }
$i = 0
$s = 0
while $i < 10000 {
    $s = $s + $m["a"]
    $s = $s + $m["b"]
    $s = $s + $m["c"]
    $s = $s + $m["d"]
    $s = $s + $m["e"]
    $s = $s + $m["f"]
    $s = $s + $m["g"]
    $s = $s + $m["h"]
    $i = $i + 1
}
$s
"#,
        "local m = {a=1,b=2,c=3,d=4,e=5,f=6,g=7,h=8}; local i = 0; local s = 0; while i < 10000 do s = s + m['a']; s = s + m['b']; s = s + m['c']; s = s + m['d']; s = s + m['e']; s = s + m['f']; s = s + m['g']; s = s + m['h']; i = i + 1 end; return s",
        360_000.0,
    );

    // 9. map_create_1k
    // Loop 1000 creating {a:$i,b:$i+1,c:$i+2,d:$i+3} and adding $m.d
    // $m.d = $i + 3, sum for i=0..999 = sum(3..1002) = sum(0..1002) - sum(0..2) = 503*1002 - 3 = 504006 - 3 = 504003
    // Wait: sum of (i+3) for i=0..999 = sum(i) + 3*1000 = 499500 + 3000 = 502500
    bench_pair(
        &mut group,
        "map_create_1k",
        r#"
$i = 0
$s = 0
while $i < 1000 {
    $m = { a: $i, b: ($i + 1), c: ($i + 2), d: ($i + 3) }
    $s = $s + $m.d
    $i = $i + 1
}
$s
"#,
        "local i = 0; local s = 0; while i < 1000 do local m = {a=i, b=i+1, c=i+2, d=i+3}; s = s + m.d; i = i + 1 end; return s",
        502_500.0,
    );

    // 10. map_of_arrays_1k
    // Map {arr: [0..99]}; loop 10 summing $m.arr[$j]
    // Sum of 0..99 = 4950, times 10 = 49500
    bench_pair(
        &mut group,
        "map_of_arrays_1k",
        r#"
$arr = []
$k = 0
while $k < 100 {
    $arr = $arr + [$k]
    $k = $k + 1
}
$m = { arr: $arr }
$s = 0
$r = 0
while $r < 10 {
    $j = 0
    while $j < 100 {
        $s = $s + $m.arr[$j]
        $j = $j + 1
    }
    $r = $r + 1
}
$s
"#,
        "local arr = {}; local k = 0; while k < 100 do table.insert(arr, k); k = k + 1 end; local m = {arr = arr}; local s = 0; local r = 0; while r < 10 do local j = 1; while j <= 100 do s = s + m.arr[j]; j = j + 1 end; r = r + 1 end; return s",
        49_500.0,
    );

    group.finish();
}

// ─── Mixed benchmarks ───

fn bench_mixed(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("mixed");

    // 11. map_nested_read_1k
    // Map of maps {inner: {a:1,b:2,c:3}}; loop 1000 summing fields
    // Per iteration: 1+2+3 = 6, total = 6000
    bench_pair(
        &mut group,
        "map_nested_read_1k",
        r#"
$m = { inner: { a: 1, b: 2, c: 3 } }
$i = 0
$s = 0
while $i < 1000 {
    $s = $s + $m.inner.a
    $s = $s + $m.inner.b
    $s = $s + $m.inner.c
    $i = $i + 1
}
$s
"#,
        "local m = {inner = {a=1, b=2, c=3}}; local i = 0; local s = 0; while i < 1000 do s = s + m.inner.a; s = s + m.inner.b; s = s + m.inner.c; i = i + 1 end; return s",
        6000.0,
    );

    // 12. array_push_sum_1k
    // Push $i 1000 times, then sum with for $x in $a
    // Sum of 0..999 = 499500
    bench_pair(
        &mut group,
        "array_push_sum_1k",
        r#"
$a = []
$i = 0
while $i < 1000 {
    $a = $a + [$i]
    $i = $i + 1
}
$s = 0
for $x in $a {
    $s = $s + $x
}
$s
"#,
        "local a = {}; local i = 0; while i < 1000 do table.insert(a, i); i = i + 1 end; local s = 0; for _, v in ipairs(a) do s = s + v end; return s",
        499_500.0,
    );

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(30)
        .warm_up_time(std::time::Duration::from_millis(300))
        .measurement_time(std::time::Duration::from_secs(2));
    targets = bench_arrays, bench_maps, bench_mixed
}
criterion_main!(benches);
