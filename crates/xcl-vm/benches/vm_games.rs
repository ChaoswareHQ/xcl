//! Pure-VM game benchmarks: life, physics, graphics.
//!
//! Compares XCL VM execution against Luau via mlua.

mod common;

use common::*;
use criterion::{BenchmarkGroup, Criterion, criterion_group, criterion_main};

// ─── Reference computations ───
// (All references are computed inline in each benchmark function)

// ─── Game benchmarks ───

fn bench_game_life(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("game/life");

    // Game of Life on a 32x32 torus for 40 generations. The grid is rebuilt each
    // generation with array append, so this exercises allocation + indexed reads.
    const W: i64 = 32;
    const H: i64 = 32;
    const GENS: i64 = 40;
    let n = (W * H) as usize;

    let ref_life = || -> i64 {
        let mut g: Vec<i64> = (0..n)
            .map(|i| if ((i as i64 * 7 + 3) % 11) < 3 { 1 } else { 0 })
            .collect();
        let mut next = vec![0i64; n];
        for _ in 0..GENS {
            for i in 0..n {
                let y = i as i64 / W;
                let x = i as i64 % W;
                let mut cnt = 0i64;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let yy = (y + dy + H) % H;
                        let xx = (x + dx + W) % W;
                        cnt += g[(yy * W + xx) as usize];
                    }
                }
                let cur = g[i];
                next[i] = if cur == 1 {
                    i64::from(cnt == 2 || cnt == 3)
                } else {
                    i64::from(cnt == 3)
                };
            }
            g.copy_from_slice(&next);
        }
        g.iter().sum()
    };
    let expected_life = ref_life() as f64;

    let grid_lit = (0..n)
        .map(|i| {
            if ((i as i64 * 7 + 3) % 11) < 3 {
                "1"
            } else {
                "0"
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let luau_grid = (0..n)
        .map(|i| {
            let v = if ((i as i64 * 7 + 3) % 11) < 3 { 1 } else { 0 };
            format!("[{i}]={v}")
        })
        .collect::<Vec<_>>()
        .join(",");

    let xcl_src = format!(
        r#"
$g = [{grid_lit}]
$gen = 0
while $gen < {GENS} {{
    $next = []
    $i = 0
    while $i < {n} {{
        $y = $i / {W}
        $x = $i % {W}
        $cnt = 0
        $dy = -1
        while $dy <= 1 {{
            $dx = -1
            while $dx <= 1 {{
                if $dy != 0 {{
                    $yy = ($y + $dy + {H}) % {H}
                    $xx = ($x + $dx + {W}) % {W}
                    $cnt = $cnt + $g[$yy * {W} + $xx]
                }} else {{
                    if $dx != 0 {{
                        $yy = ($y + $dy + {H}) % {H}
                        $xx = ($x + $dx + {W}) % {W}
                        $cnt = $cnt + $g[$yy * {W} + $xx]
                    }}
                }}
                $dx = $dx + 1
            }}
            $dy = $dy + 1
        }}
        $alive = 0
        $cur = $g[$i]
        if $cur == 1 {{
            if $cnt == 2 {{ $alive = 1 }}
            if $cnt == 3 {{ $alive = 1 }}
        }} else {{
            if $cnt == 3 {{ $alive = 1 }}
        }}
        $next = $next + [$alive]
        $i = $i + 1
    }}
    $g = $next
    $gen = $gen + 1
}}
$s = 0
$i = 0
while $i < {n} {{
    $s = $s + $g[$i]
    $i = $i + 1
}}
$s
"#
    );

    let luau_src = format!(
        r#"
local g = {{{luau_grid}}}
local gen = 0
while gen < {GENS} do
    local next = {{}}
    local i = 0
    while i < {n} do
        local y = math.floor(i / {W})
        local x = i % {W}
        local cnt = 0
        local dy = -1
        while dy <= 1 do
            local dx = -1
            while dx <= 1 do
                if dy ~= 0 then
                    local yy = (y + dy + {H}) % {H}
                    local xx = (x + dx + {W}) % {W}
                    cnt = cnt + g[yy * {W} + xx]
                else
                    if dx ~= 0 then
                        local yy = (y + dy + {H}) % {H}
                        local xx = (x + dx + {W}) % {W}
                        cnt = cnt + g[yy * {W} + xx]
                    end
                end
                dx = dx + 1
            end
            dy = dy + 1
        end
        local alive = 0
        local cur = g[i]
        if cur == 1 then
            if cnt == 2 then alive = 1 end
            if cnt == 3 then alive = 1 end
        else
            if cnt == 3 then alive = 1 end
        end
        next[i] = alive
        i = i + 1
    end
    g = next
    gen = gen + 1
end
local s = 0
local i = 0
while i < {n} do
    s = s + g[i]
    i = i + 1
end
return s
"#
    );

    bench_pair(
        &mut group,
        "game_of_life_32x32_40",
        &xcl_src,
        &luau_src,
        expected_life,
    );

    // 1D cellular wave: new[i] = (old[i-1] + old[i] + old[i+1]) % 3, boundary 0.
    // 500 steps on a 128-cell array, return the final checksum.

    let ref_wave = || -> i64 {
        let mut grid: Vec<i64> = vec![0i64; 128];
        grid[64] = 1;
        for _ in 0..500 {
            let mut next = vec![0i64; 128];
            for i in 0..128 {
                let left = if i == 0 { 0 } else { grid[i - 1] };
                let center = grid[i];
                let right = if i == 127 { 0 } else { grid[i + 1] };
                next[i] = (left + center + right) % 3;
            }
            grid = next;
        }
        let mut checksum: i64 = 0;
        for (i, &v) in grid.iter().enumerate() {
            checksum = (checksum.wrapping_mul(31).wrapping_add(v + i as i64)) % 1000003;
        }
        checksum
    };
    let expected_wave = ref_wave() as f64;

    // Build the initial grid: 128 zeros with index 64 = 1
    let mut grid_vals: Vec<String> = vec!["0".to_string(); 128];
    grid_vals[64] = "1".to_string();
    let grid_lit = grid_vals.join(", ");

    let xcl_src = format!(
        r#"
wave_step($grid) {{
    $next = []
    $i = 0
    while $i < 128 {{
        $left = 0
        if $i > 0 {{
            $left = $grid[$i - 1]
        }}
        $center = $grid[$i]
        $right = 0
        if $i < 127 {{
            $right = $grid[$i + 1]
        }}
        $val = ($left + $center + $right) % 3
        $next = $next + [$val]
        $i = $i + 1
    }}
    return $next
}}
checksum($grid) {{
    $cs = 0
    $i = 0
    while $i < 128 {{
        $cs = (($cs * 31 + $grid[$i] + $i) % 1000003)
        $i = $i + 1
    }}
    return $cs
}}
$grid = [{grid}]
$gen = 0
while $gen < 500 {{
    $next = wave_step $grid
    $grid = $next
    $gen = $gen + 1
}}
$count = checksum $grid
$count
"#,
        grid = grid_lit
    );

    // Luau version (1-based)
    let luau_grid = grid_vals.join(",");
    let luau_src = format!(
        r#"
local function wave_step(grid)
    local next = {{}};
    local i = 1;
    while i <= 128 do
        local left = (i == 1) and 0 or grid[i - 1];
        local center = grid[i];
        local right = (i == 128) and 0 or grid[i + 1];
        table.insert(next, (left + center + right) % 3);
        i = i + 1;
    end;
    return next;
end;
local function checksum(grid)
    local cs = 0;
    local i = 1;
    while i <= 128 do
        cs = (cs * 31 + grid[i] + (i - 1)) % 1000003;
        i = i + 1;
    end;
    return cs;
end;
local grid = {{{grid}}};
local gen = 0;
while gen < 500 do
    grid = wave_step(grid);
    gen = gen + 1;
end;
return checksum(grid)
"#,
        grid = luau_grid
    );

    bench_pair(
        &mut group,
        "wave_1d_128x500",
        &xcl_src,
        &luau_src,
        expected_wave,
    );

    group.finish();
}

fn bench_game_physics(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("game/physics");

    // LCG with a 48-bit-safe multiplier/modulus (100k iterations).
    let ref_lcg = || -> i64 {
        let mut x: i64 = 42;
        for _ in 0..100000i64 {
            x = (x * 37 + 12345) % 1000003;
        }
        x
    };
    let expected_lcg = ref_lcg() as f64;

    bench_pair(
        &mut group,
        "lcg_100k",
        r#"
$x = 42
$i = 0
while $i < 100000 {
    $x = ($x * 37 + 12345) % 1000003
    $i = $i + 1
}
$x
"#,
        "local x = 42; local i = 0; while i < 100000 do x = (x * 37 + 12345) % 1000003; i = i + 1 end; return x",
        expected_lcg,
    );

    group.finish();
}

fn bench_game_graphics(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("game/graphics");

    // 32x32 procedural texture hash (game graphics category).

    // Reference: hash a 32x32 texture
    let ref_texture_hash = || -> i64 {
        let mut hash: i64 = 0;
        for y in 0..32 {
            let mut row_hash: i64 = 0;
            for x in 0..32 {
                let val = ((x * 7 + y * 13 + (x * y) % 17) % 256) as i64;
                row_hash = (row_hash.wrapping_mul(31).wrapping_add(val)) % 1000003;
            }
            hash = (hash.wrapping_mul(31).wrapping_add(row_hash)) % 1000003;
        }
        hash
    };
    let expected_texture = ref_texture_hash() as f64;

    bench_pair(
        &mut group,
        "texture_hash_32x32",
        r#"
hash_row($y) {
    $hash = 0
    $x = 0
    while $x < 32 {
        $val = (($x * 7 + $y * 13 + ($x * $y) % 17) % 256)
        $hash = (($hash * 31 + $val) % 1000003)
        $x = $x + 1
    }
    return $hash
}
$hash = 0
$y = 0
while $y < 32 {
    $rh = hash_row $y
    $hash = (($hash * 31 + $rh) % 1000003)
    $y = $y + 1
}
$hash
"#,
        "local function hash_row(y) local hash = 0; local x = 0; while x < 32 do local val = (x * 7 + y * 13 + (x * y) % 17) % 256; hash = (hash * 31 + val) % 1000003; x = x + 1 end; return hash end; local hash = 0; local y = 0; while y < 32 do local rh = hash_row(y); hash = (hash * 31 + rh) % 1000003; y = y + 1 end; return hash",
        expected_texture,
    );

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(30)
        .warm_up_time(std::time::Duration::from_millis(300))
        .measurement_time(std::time::Duration::from_secs(2));
    targets = bench_game_life, bench_game_physics, bench_game_graphics
}
criterion_main!(benches);
