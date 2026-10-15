# XCL Baseline Profile

Generated: 2026-10-15 21:10:48
Commit: faa5705

## Benchmark Results

     Running benches\dispatch.rs (target\release\deps\dispatch-e256ab7bfc90579f.exe)
Gnuplot not found, using plotters backend
Benchmarking arithmetic_loop_10k
Benchmarking arithmetic_loop_10k: Warming up for 3.0000 s
Benchmarking arithmetic_loop_10k: Collecting 100 samples in estimated 5.7040 s (10k iterations)
Benchmarking arithmetic_loop_10k: Analyzing
arithmetic_loop_10k     time:   [568.86 µs 581.45 µs 596.23 µs]
                        change: [+5.0907% +8.9688% +13.240%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 11 outliers among 100 measurements (11.00%)
  3 (3.00%) high mild
  8 (8.00%) high severe

=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 2378012910
Opcode              Count        %
----------------------------------
OP_PUSH_INT     548784873   23.08%
OP_LOAD_LOCAL    548766582   23.08%
OP_STORE_LOCAL    365856582   15.38%
OP_ADD          365820000   15.38%
OP_LT           182928291    7.69%
OP_JUMP_IF_FALSE    182928291    7.69%
OP_JUMP         182910000    7.69%
OP_HALT             18291    0.00%
====================================
Benchmarking closure_call_5k
Benchmarking closure_call_5k: Warming up for 3.0000 s
Benchmarking closure_call_5k: Collecting 100 samples in estimated 5.2063 s (20k iterations)
Benchmarking closure_call_5k: Analyzing
closure_call_5k         time:   [231.73 µs 234.78 µs 238.03 µs]
                        change: [-7.8373% -5.8173% -3.6390%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 4 outliers among 100 measurements (4.00%)
  1 (1.00%) low mild
  2 (2.00%) high mild
  1 (1.00%) high severe

=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 2012503996
Opcode              Count        %
----------------------------------
OP_PUSH_INT     365939749   18.18%
OP_STORE_LOCAL    365939749   18.18%
OP_LOAD_LOCAL    365903166   18.18%
OP_LT           182951583    9.09%
OP_JUMP_IF_FALSE    182951583    9.09%
OP_ADD          182915000    9.09%
OP_JUMP         182915000    9.09%
OP_LL_LL        182915000    9.09%
OP_HALT             36583    0.00%
OP_LOAD_LAMBDA        36583    0.00%
====================================
Benchmarking closure_capture_5k
Benchmarking closure_capture_5k: Warming up for 3.0000 s
Benchmarking closure_capture_5k: Collecting 100 samples in estimated 6.1435 s (25k iterations)
Benchmarking closure_capture_5k: Analyzing
closure_capture_5k      time:   [233.97 µs 236.80 µs 239.90 µs]
                        change: [-4.7517% -2.6502% -0.6140%] (p = 0.01 < 0.05)
                        Change within noise threshold.
Found 6 outliers among 100 measurements (6.00%)
  5 (5.00%) high mild
  1 (1.00%) high severe

=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 2290397862
Opcode              Count        %
----------------------------------
OP_PUSH_INT     416496532   18.18%
OP_STORE_LOCAL    416496532   18.18%
OP_LOAD_LOCAL    416413266   18.18%
OP_LT           208206633    9.09%
OP_JUMP_IF_FALSE    208206633    9.09%
OP_ADD          208165000    9.09%
OP_JUMP         208165000    9.09%
OP_LL_LL        208165000    9.09%
OP_HALT             41633    0.00%
OP_LOAD_LAMBDA        41633    0.00%
====================================
Benchmarking array_iteration_10k_items
Benchmarking array_iteration_10k_items: Warming up for 3.0000 s
Benchmarking array_iteration_10k_items: Collecting 100 samples in estimated 5.0953 s (15k iterations)
Benchmarking array_iteration_10k_items: Analyzing
array_iteration_10k_items
                        time:   [395.63 µs 410.09 µs 425.26 µs]
                        change: [+14.912% +18.624% +22.518%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 1 outliers among 100 measurements (1.00%)
  1 (1.00%) high mild

=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 2271069726
Opcode              Count        %
----------------------------------
OP_STORE_LOCAL    662287599   29.16%
OP_ADD          346863000   15.27%
OP_JUMP         346863000   15.27%
OP_FOR_ITER_NEXT    346863000   15.27%
OP_LL_LL        315330000   13.88%
OP_LOAD_LOCAL     94662066    4.17%
OP_PUSH_INT      63475929    2.79%
OP_LT            31564533    1.39%
OP_JUMP_IF_FALSE     31564533    1.39%
OP_FOR_ITER_START     31533000    1.39%
OP_HALT             31533    0.00%
OP_MAKE_ARRAY        31533    0.00%
====================================
Benchmarking range_loop_10k
Benchmarking range_loop_10k: Warming up for 3.0000 s
Benchmarking range_loop_10k: Collecting 100 samples in estimated 5.2357 s (20k iterations)
Benchmarking range_loop_10k: Analyzing
range_loop_10k          time:   [247.28 µs 251.46 µs 255.92 µs]
                        change: [-0.8395% +2.1676% +5.3931%] (p = 0.17 > 0.05)
                        No change in performance detected.
Found 5 outliers among 100 measurements (5.00%)
  4 (4.00%) high mild
  1 (1.00%) high severe

=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 2195272664
Opcode              Count        %
----------------------------------
OP_STORE_LOCAL    731696583   33.33%
OP_FOR_RANGE_NEXT    365866583   16.67%
OP_ADD          365830000   16.66%
OP_JUMP         365830000   16.66%
OP_LL_LL        365830000   16.66%
OP_PUSH_INT        109749    0.00%
OP_LOAD_LOCAL        36583    0.00%
OP_HALT             36583    0.00%
OP_FOR_RANGE_START        36583    0.00%
====================================
Benchmarking dict_access_10k
Benchmarking dict_access_10k: Warming up for 3.0000 s
Benchmarking dict_access_10k: Collecting 100 samples in estimated 5.0532 s (1500 iterations)
Benchmarking dict_access_10k: Analyzing
dict_access_10k         time:   [3.2393 ms 3.3037 ms 3.3739 ms]
                        change: [-3.6451% -0.7788% +2.0630%] (p = 0.61 > 0.05)
                        No change in performance detected.
Found 13 outliers among 100 measurements (13.00%)
  11 (11.00%) high mild
  2 (2.00%) high severe

=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 857890644
Opcode              Count        %
----------------------------------
OP_LOAD_LOCAL    227075046   26.47%
OP_ADD          227070000   26.47%
OP_GET_FIELD    201840000   23.53%
OP_PUSH_INT      50487753    5.89%
OP_STORE_LOCAL     50467569    5.88%
OP_LT            25232523    2.94%
OP_JUMP_IF_FALSE     25232523    2.94%
OP_JUMP          25230000    2.94%
OP_LL_LL         25230000    2.94%
OP_PUSH_STR         20184    0.00%
OP_HALT              2523    0.00%
OP_MAKE_DICT         2523    0.00%
====================================
Benchmarking branching_10k
Benchmarking branching_10k: Warming up for 3.0000 s
Benchmarking branching_10k: Collecting 100 samples in estimated 7.1169 s (10k iterations)
Benchmarking branching_10k: Analyzing
branching_10k           time:   [697.92 µs 710.65 µs 724.55 µs]
                        change: [-5.4838% -0.7690% +3.0527%] (p = 0.77 > 0.05)
                        No change in performance detected.
Found 9 outliers among 100 measurements (9.00%)
  2 (2.00%) low mild
  6 (6.00%) high mild
  1 (1.00%) high severe

=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 3109652910
Opcode              Count        %
----------------------------------
OP_PUSH_INT     823149873   26.47%
OP_LOAD_LOCAL    640221582   20.59%
OP_JUMP_IF_FALSE    365838291   11.76%
OP_STORE_LOCAL    274401582    8.82%
OP_ADD          274365000    8.82%
OP_LT           182928291    5.88%
OP_MOD          182910000    5.88%
OP_EQ           182910000    5.88%
OP_JUMP         182910000    5.88%
OP_HALT             18291    0.00%
====================================
Benchmarking nested_closure_2k
Benchmarking nested_closure_2k: Warming up for 3.0000 s
Benchmarking nested_closure_2k: Collecting 100 samples in estimated 5.4004 s (35k iterations)
Benchmarking nested_closure_2k: Analyzing
nested_closure_2k       time:   [124.85 µs 129.97 µs 135.77 µs]
                        change: [+14.013% +19.090% +24.006%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 9 outliers among 100 measurements (9.00%)
  6 (6.00%) high mild
  3 (3.00%) high severe

=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 1499800106
Opcode              Count        %
----------------------------------
OP_STORE_LOCAL    272808585   18.19%
OP_LOAD_LOCAL    272740468   18.19%
OP_PUSH_INT     272672351   18.18%
OP_LT           136302117    9.09%
OP_JUMP_IF_FALSE    136302117    9.09%
OP_ADD          136234000    9.08%
OP_JUMP         136234000    9.08%
OP_LL_LL        136234000    9.08%
OP_LOAD_LAMBDA       204351    0.01%
OP_HALT             68117    0.00%
====================================
Benchmarking string_concat_1k
Benchmarking string_concat_1k: Warming up for 3.0000 s
Benchmarking string_concat_1k: Collecting 100 samples in estimated 8.2611 s (10k iterations)
Benchmarking string_concat_1k: Analyzing
string_concat_1k        time:   [1.0364 ms 1.1755 ms 1.3419 ms]
                        change: [-2.3393% +7.8150% +18.390%] (p = 0.13 > 0.05)
                        No change in performance detected.
Found 2 outliers among 100 measurements (2.00%)
  1 (1.00%) high mild
  1 (1.00%) high severe

=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 238002492
Opcode              Count        %
----------------------------------
OP_LOAD_LOCAL     54909582   23.07%
OP_PUSH_INT      36618582   15.39%
OP_STORE_LOCAL     36618582   15.39%
OP_ADD           36582000   15.37%
OP_PUSH_STR      18327582    7.70%
OP_LT            18309291    7.69%
OP_JUMP_IF_FALSE     18309291    7.69%
OP_JUMP          18291000    7.69%
OP_CALL             18291    0.01%
OP_HALT             18291    0.01%
====================================


## Opcode Histogram

Run with: cargo bench -p xcl-vm --features opcode-stats -- --noplot

## Flamegraph

Path: C:\Users\AzzoDude\Documents\workspace\xcl\state\profile.json.gz

On Windows, cargo-flamegraph produces unreadable flamegraphs (raw memory addresses).
Use **samply** instead for proper symbol resolution:

1. Install: cargo install --locked samply
2. Profile: .\scripts\profile.ps1 -Flamegraph
3. View: Open the saved profile.json.gz in https://profiler.firefox.com/
   Or run: samply replay state/profile.json.gz

Firefox Profiler shows an interactive flamegraph with resolved function names,
source code links, and call tree analysis.

## Build Profiles

- **bench-flamegraph**: Debug info + opt-level 1 for flamegraph readability
- **release-lto-debug**: LTO release with debuginfo for BOLT re-compilation

## How to Use

1. Run benchmarks: cargo bench -p xcl-vm
2. Run with opcode stats: cargo bench -p xcl-vm --features opcode-stats
3. Generate flamegraph: .\scripts\profile.ps1 -Flamegraph (uses samply)
4. Compare after optimization: re-run and diff results