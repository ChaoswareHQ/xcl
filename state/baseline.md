# XCL Baseline Profile

Generated: 2026-10-15 20:32:57
Commit: 4d506c3

## Benchmark Results

cargo :     Finished `bench` profile [optimized] target(s) in 0.31s
At C:\Users\AzzoDude\Documents\workspace\xcl\scripts\profile.ps1:41 char:20
+ ... nchOutput = cargo bench -p xcl-vm --features opcode-stats --bench dis ...
+                 ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
    + CategoryInfo          : NotSpecified: (    Finished `b...get(s) in 0.31s:String) [], RemoteException
    + FullyQualifiedErrorId : NativeCommandError
 
     Running benches\dispatch.rs (target\release\deps\dispatch-e256ab7bfc90579f.exe)
Gnuplot not found, using plotters backend
Benchmarking arithmetic_loop_10k
Benchmarking arithmetic_loop_10k: Warming up for 3.0000 s
Benchmarking arithmetic_loop_10k: Collecting 100 samples in estimated 5.9167 s (10k iterations)
Benchmarking arithmetic_loop_10k: Analyzing
arithmetic_loop_10k     time:   [675.87 µs 714.96 µs 754.84 µs]
                        change: [+30.201% +35.488% +41.308%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 3 outliers among 100 measurements (3.00%)
  3 (3.00%) high mild


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
Benchmarking closure_call_5k: Collecting 100 samples in estimated 6.1373 s (25k iterations)
Benchmarking closure_call_5k: Analyzing
closure_call_5k         time:   [242.05 µs 246.95 µs 252.04 µs]
                        change: [+12.499% +15.953% +19.230%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 6 outliers among 100 measurements (6.00%)
  5 (5.00%) high mild
  1 (1.00%) high severe


=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 2290314596
Opcode              Count        %
----------------------------------
OP_PUSH_INT     416454899   18.18%
OP_STORE_LOCAL    416454899   18.18%
OP_LOAD_LOCAL    416413266   18.18%
OP_LT           208206633    9.09%
OP_JUMP_IF_FALSE    208206633    9.09%
OP_ADD          208165000    9.09%
OP_JUMP         208165000    9.09%
OP_LL_LL        208165000    9.09%
OP_HALT             41633    0.00%
OP_LOAD_LAMBDA        41633    0.00%
====================================

Benchmarking closure_capture_5k
Benchmarking closure_capture_5k: Warming up for 3.0000 s
Benchmarking closure_capture_5k: Collecting 100 samples in estimated 5.5183 s (20k iterations)
Benchmarking closure_capture_5k: Analyzing
closure_capture_5k      time:   [241.32 µs 245.38 µs 249.81 µs]
                        change: [+9.9626% +13.268% +16.611%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 5 outliers among 100 measurements (5.00%)
  1 (1.00%) low mild
  3 (3.00%) high mild
  1 (1.00%) high severe


=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 2012577162
Opcode              Count        %
----------------------------------
OP_PUSH_INT     365976332   18.18%
OP_STORE_LOCAL    365976332   18.18%
OP_LOAD_LOCAL    365903166   18.18%
OP_LT           182951583    9.09%
OP_JUMP_IF_FALSE    182951583    9.09%
OP_ADD          182915000    9.09%
OP_JUMP         182915000    9.09%
OP_LL_LL        182915000    9.09%
OP_HALT             36583    0.00%
OP_LOAD_LAMBDA        36583    0.00%
====================================

Benchmarking array_iteration_10k_items
Benchmarking array_iteration_10k_items: Warming up for 3.0000 s
Benchmarking array_iteration_10k_items: Collecting 100 samples in estimated 5.1990 s (15k iterations)
Benchmarking array_iteration_10k_items: Analyzing
array_iteration_10k_items
                        time:   [328.84 µs 335.73 µs 343.40 µs]
                        change: [+2.2890% +7.0038% +11.704%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 12 outliers among 100 measurements (12.00%)
  1 (1.00%) low mild
  5 (5.00%) high mild
  6 (6.00%) high severe


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
Benchmarking range_loop_10k: Collecting 100 samples in estimated 5.1355 s (20k iterations)
Benchmarking range_loop_10k: Analyzing
range_loop_10k          time:   [246.36 µs 251.44 µs 257.26 µs]
                        change: [-8.6969% -6.3024% -3.7580%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 100 measurements (2.00%)
  2 (2.00%) high mild


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
Benchmarking dict_access_10k: Collecting 100 samples in estimated 5.2894 s (1600 iterations)
Benchmarking dict_access_10k: Analyzing
dict_access_10k         time:   [3.3602 ms 3.4462 ms 3.5419 ms]
                        change: [+3.2862% +6.6748% +10.422%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 5 outliers among 100 measurements (5.00%)
  2 (2.00%) high mild
  3 (3.00%) high severe


=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 891893444
Opcode              Count        %
----------------------------------
OP_LOAD_LOCAL    236075246   26.47%
OP_ADD          236070000   26.47%
OP_GET_FIELD    209840000   23.53%
OP_PUSH_INT      52488853    5.89%
OP_STORE_LOCAL     52467869    5.88%
OP_LT            26232623    2.94%
OP_JUMP_IF_FALSE     26232623    2.94%
OP_JUMP          26230000    2.94%
OP_LL_LL         26230000    2.94%
OP_PUSH_STR         20984    0.00%
OP_HALT              2623    0.00%
OP_MAKE_DICT         2623    0.00%
====================================

Benchmarking branching_10k
Benchmarking branching_10k: Warming up for 3.0000 s
Benchmarking branching_10k: Collecting 100 samples in estimated 7.2360 s (10k iterations)
Benchmarking branching_10k: Analyzing
branching_10k           time:   [685.46 µs 699.40 µs 714.45 µs]
                        change: [-0.7734% +2.3518% +5.6822%] (p = 0.15 > 0.05)
                        No change in performance detected.
Found 10 outliers among 100 measurements (10.00%)
  2 (2.00%) low mild
  6 (6.00%) high mild
  2 (2.00%) high severe


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
Benchmarking nested_closure_2k: Collecting 100 samples in estimated 5.1809 s (45k iterations)
Benchmarking nested_closure_2k: Analyzing
nested_closure_2k       time:   [118.69 µs 129.14 µs 142.18 µs]
                        change: [+2.6579% +8.6645% +15.174%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 6 outliers among 100 measurements (6.00%)
  2 (2.00%) high mild
  4 (4.00%) high severe


=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 1722181906
Opcode              Count        %
----------------------------------
OP_STORE_LOCAL    313259085   18.19%
OP_LOAD_LOCAL    313180868   18.19%
OP_PUSH_INT     313102651   18.18%
OP_LT           156512217    9.09%
OP_JUMP_IF_FALSE    156512217    9.09%
OP_ADD          156434000    9.08%
OP_JUMP         156434000    9.08%
OP_LL_LL        156434000    9.08%
OP_LOAD_LAMBDA       234651    0.01%
OP_HALT             78217    0.00%
====================================

Benchmarking string_concat_1k
Benchmarking string_concat_1k: Warming up for 3.0000 s
Benchmarking string_concat_1k: Collecting 100 samples in estimated 8.5443 s (10k iterations)
Benchmarking string_concat_1k: Analyzing
string_concat_1k        time:   [921.33 µs 980.61 µs 1.0438 ms]
                        change: [-19.462% -13.395% -6.8979%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 2 outliers among 100 measurements (2.00%)
  2 (2.00%) high mild


=== Opcode Dispatch Histogram (top 15) ===
Total dispatches: 184705340
Opcode              Count        %
----------------------------------
OP_LOAD_LOCAL     42613390   23.07%
OP_PUSH_INT      28418390   15.39%
OP_STORE_LOCAL     28418390   15.39%
OP_ADD           28390000   15.37%
OP_PUSH_STR      14223390    7.70%
OP_LT            14209195    7.69%
OP_JUMP_IF_FALSE     14209195    7.69%
OP_JUMP          14195000    7.69%
OP_CALL             14195    0.01%
OP_HALT             14195    0.01%
====================================



## Opcode Histogram

Run with: cargo bench -p xcl-vm --features opcode-stats -- --noplot

## Flamegraph

Path: C:\Users\AzzoDude\Documents\workspace\xcl\state\flamegraph.svg

## Build Profiles

- **bench-flamegraph**: Debug info + opt-level 1 for flamegraph readability
- **release-lto-debug**: LTO release with debuginfo for BOLT re-compilation

## How to Use

1. Run benchmarks: cargo bench -p xcl-vm
2. Run with opcode stats: cargo bench -p xcl-vm --features opcode-stats
3. Generate flamegraph: cargo flamegraph --bench dispatch
4. Compare after optimization: re-run and diff results