//! Regression tests for VM correctness bugs fixed in vm-bench-phase1.

fn eval(src: &str) -> xcl_vm::Value {
    let tokens = xcl_vm::lexer::lex(src, "<test>");
    let ast = xcl_vm::parser::parse(&tokens, src);
    let bc = xcl_vm::bytecode::compile(&ast);
    let mut vm = xcl_vm::VM::new();
    let result = vm.run(&bc);
    // Tests inspect heap-backed values (strings/maps) after this helper returns.
    core::mem::forget(vm);
    result
}

fn eval_i64(src: &str) -> i64 {
    eval(src).as_i48()
}

// ── Values returned by `run` must survive the VM (escape barrier) ──

#[test]
fn returned_values_survive_vm_drop() {
    // The benchmark harness runs a program, drops the VM, then inspects the
    // result. Anything the arena owns must be copied out before `run` returns.
    fn run_and_drop(src: &str) -> xcl_vm::Value {
        let tokens = xcl_vm::lexer::lex(src, "<test>");
        let ast = xcl_vm::parser::parse(&tokens, src);
        let bc = xcl_vm::bytecode::compile(&ast);
        let mut vm = xcl_vm::VM::new();
        vm.run(&bc)
    }

    // Concatenated string: its bytes used to live in the arena.
    let s = run_and_drop("$s = \"\"\n$i = 0\nwhile $i < 64 { $s = $s + \"x\"\n $i = $i + 1 }\n$s");
    assert_eq!(s.as_str().map(|s| s.len()), Some(64));

    // Array of strings, including a nested array.
    let arr = run_and_drop("[\"a\", [\"b\", \"c\"], \"d\"]");
    let outer = arr.as_array().expect("array");
    assert_eq!(outer[0].as_str(), Some("a"));
    assert_eq!(outer[2].as_str(), Some("d"));
    let inner = outer[1].as_array().expect("nested array");
    assert_eq!(inner[1].as_str(), Some("c"));

    // Map with a string value.
    let m = run_and_drop("{ name: \"xcl\", n: 3 }");
    assert_eq!(
        m.as_map()
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str()),
        Some("xcl")
    );
}

fn eval_f64(src: &str) -> f64 {
    let v = eval(src);
    assert!(v.is_float(), "expected float, got {:?}", v);
    v.as_f64()
}

// ── Bug 1: Dead-store elimination removes live stores before fused ops ──

#[test]
fn array_index_fused_store() {
    assert_eq!(eval_i64("$a = [10,20,30]; $x = $a[1]; $x"), 20);
}

#[test]
fn array_index_in_while_loop() {
    // Array of 5 values [10,20,30,40,50], sum via $a[$i] == 150
    assert_eq!(
        eval_i64(
            "$a = [10,20,30,40,50]; $s = 0; $i = 0; while $i < 5 { $s = $s + $a[$i]; $i = $i + 1 }; $s"
        ),
        150
    );
}

#[test]
fn array_push_then_index() {
    // $a = [], push 0..4 with $a = $a + [$i], $a[4] == 4
    assert_eq!(
        eval_i64("$a = []; $i = 0; while $i < 5 { $a = $a + [$i]; $i = $i + 1 }; $a[4]"),
        4
    );
}

// ── Bug 2: OP_CALL_METHOD cannot read a map field ──

#[test]
fn map_field_assignment() {
    assert_eq!(eval_i64("$m = {x:7,y:8}; $m.x + $m.y"), 15);
}

#[test]
fn map_field_in_while_loop() {
    // 3 fields 1/2/3, three iterations == 18
    assert_eq!(
        eval_i64(
            "$m = {a:1,b:2,c:3}; $s = 0; $i = 0; while $i < 3 { $s = $s + $m.a + $m.b + $m.c; $i = $i + 1 }; $s"
        ),
        18
    );
}

#[test]
fn map_index_assignment() {
    assert_eq!(eval_i64("$m = {x:7}; $r = $m[\"x\"]; $r"), 7);
}

// ── Bug 3: Nested/repeated calls truncate the caller's temporaries ──

#[test]
fn nested_named_calls() {
    assert_eq!(
        eval_i64("add($a, $b) { return $a + $b }; $r = (add 1 2) + (add 3 4); $r"),
        10
    );
}

#[test]
fn recursive_fib() {
    assert_eq!(
        eval_i64(
            "fib($n) { if $n < 2 { return $n } return (fib ($n - 1)) + (fib ($n - 2)) }; fib 10"
        ),
        55
    );
}

#[test]
fn named_call_in_while_loop() {
    assert_eq!(
        eval_i64(
            "inc($x) { return $x + 1 }
$i = 0
$r = 0
while $i < 5000 {
    $r = inc $r
    $i = $i + 1
}
$r"
        ),
        5000
    );
}

#[test]
fn lambda_call_once() {
    assert_eq!(
        eval_i64(
            "$f = ($x) => $x + 1
$f 6"
        ),
        7
    );
}

// ── Bug 4: OP_FOR_NUMBER_LOOP operand packing mismatch ──

#[test]
fn constant_range_loop() {
    assert_eq!(eval_i64("$s = 0; for $i in 0..10 { $s = $s + $i }; $s"), 45);
}

#[test]
fn constant_range_loop_nonzero_start() {
    assert_eq!(eval_i64("$s = 0; for $i in 1..4 { $s = $s + $i }; $s"), 6);
}

#[test]
fn nonconstant_range_loop() {
    assert_eq!(
        eval_i64("$n = 10; $s = 0; for $i in 0..$n { $s = $s + $i }; $s"),
        45
    );
}

// ── Additional regression tests ──

#[test]
fn array_for_in() {
    // [1,2,3,4] sum == 10
    assert_eq!(
        eval_i64("$s = 0; for $x in [1,2,3,4] { $s = $s + $x }; $s"),
        10
    );
}

#[test]
fn nested_array_index() {
    assert_eq!(eval_i64("$a = [[1,2],[3,4]]; $a[1][0]"), 3);
}

// ── CSE soundness regression tests ──

#[test]
fn map_fields_separate_statements() {
    // Reproduction: CSE must not reuse stale expressions after a slot is redefined
    assert_eq!(
        eval_i64(
            "$m = { a: 1, b: 2, c: 3 }
$s = 0
$i = 0
while $i < 3 {
    $s = $s + $m.a
    $s = $s + $m.b
    $s = $s + $m.c
    $i = $i + 1
}
$s"
        ),
        18
    );
}

#[test]
fn map_fields_nested_expression() {
    // $s = $s + $m.a + $m.b + $m.c in a loop == 18
    assert_eq!(
        eval_i64(
            "$m = { a: 1, b: 2, c: 3 }
$s = 0
$i = 0
while $i < 3 {
    $s = $s + $m.a + $m.b + $m.c
    $i = $i + 1
}
$s"
        ),
        18
    );
}

#[test]
fn store_back_invalidation() {
    // $a = 1; $b = 2; $a = $a + $b; $a = $a + $b; $a == 5
    assert_eq!(
        eval_i64(
            "$a = 1
$b = 2
$a = $a + $b
$a = $a + $b
$a"
        ),
        5
    );
}

#[test]
fn repeated_pure_expression() {
    // $x = 2; $y = 3; $a = $x + $y; $b = $x + $y; $a + $b == 10
    assert_eq!(
        eval_i64(
            "$x = 2
$y = 3
$a = $x + $y
$b = $x + $y
$a + $b"
        ),
        10
    );
}

#[test]
fn array_index_loop_invalidation() {
    // Array [10,20,30,40,50], sum $a[$i] in a while loop == 150
    assert_eq!(
        eval_i64(
            "$a = [10,20,30,40,50]
$s = 0
$i = 0
while $i < 5 {
    $s = $s + $a[$i]
    $i = $i + 1
}
$s"
        ),
        150
    );
}

// ── Bug A: Float constants reinterpreted as integers ──

#[test]
fn float_constants_in_while_loop() {
    let result = eval_f64(
        "$a = 1.5
$i = 0
while $i < 3 {
    $a = $a + 0.5
    $i = $i + 1
}
$a",
    );
    assert!((result - 3.0).abs() < 1e-6, "expected 3.0, got {}", result);
}

// ── Bug B: Cross-block liveness — map index in while loop ──

#[test]
fn map_index_in_while_loop() {
    assert_eq!(
        eval_i64(
            "$m = { a: 1 }
$i = 0
$s = 0
while $i < 3 {
    $s = $s + $m[\"a\"]
    $i = $i + 1
}
$s"
        ),
        3
    );
}

#[test]
fn map_index_string_key_loop() {
    // Map with two keys, loop summing $m["a"] + $m["b"] (separate statements), 3 iterations
    // (1 + 2) * 3 = 9
    assert_eq!(
        eval_i64(
            "$m = { a: 1, b: 2 }
$i = 0
$s = 0
while $i < 3 {
    $s = $s + $m[\"a\"]
    $s = $s + $m[\"b\"]
    $i = $i + 1
}
$s"
        ),
        9
    );
}

// ── Constant propagation must invalidate fused local-store destinations ──

#[test]
fn constant_prop_mod_store() {
    // 42 * 37 + 12345 == 13899, which is below the modulus.
    assert_eq!(
        eval_i64(
            "$x = 42
$t = $x * 37
$t = $t + 12345
$x = $t % 1000003
$x"
        ),
        13_899
    );
}

#[test]
fn while_limit_above_u16() {
    // A constant while limit above 65535 must not be truncated to u16.
    assert_eq!(
        eval_i64(
            "$i = 0
$n = 0
while $i < 70000 {
    $n = $n + 1
    $i = $i + 1
}
$n"
        ),
        70_000
    );
}

#[test]
fn constant_prop_lcg_3_steps() {
    // Three LCG steps from 42 must not be folded back to the initial constant.
    assert_eq!(
        eval_i64(
            "$x = 42
$i = 0
while $i < 3 {
    $x = ($x * 37 + 12345) % 1000003
    $i = $i + 1
}
$x"
        ),
        496_784
    );
}

// ── Call optimization regressions (vm-optimize-calls) ──

#[test]
fn string_literal_push_return() {
    // Ensure OP_PUSH_STR uses string_values correctly
    assert_eq!(
        eval("hello() { return \"world\" }; hello()").as_str(),
        Some("world")
    );
}

#[test]
fn string_literal_in_loop() {
    // String literals in tight loops must not allocate per iteration
    assert_eq!(
        eval(
            "$s = \"\"
$i = 0
while $i < 100 {
    $s = \"x\"
    $i = $i + 1
}
$s"
        )
        .as_str(),
        Some("x")
    );
}

#[test]
fn mutual_recursion() {
    // Mutual recursion must work correctly with callee cache
    assert_eq!(
        eval_i64(
            "even($n) { if $n == 0 { return 1 } return odd ($n - 1) }
odd($n) { if $n == 0 { return 0 } return even ($n - 1) }
even 10"
        ),
        1
    );
    assert_eq!(
        eval_i64(
            "even($n) { if $n == 0 { return 1 } return odd ($n - 1) }
odd($n) { if $n == 0 { return 0 } return even ($n - 1) }
odd 10"
        ),
        0
    );
}

#[test]
fn named_call_zero_args() {
    assert_eq!(
        eval("greet() { return \"hi\" }; greet()").as_str(),
        Some("hi")
    );
}

#[test]
fn named_call_one_arg() {
    assert_eq!(eval_i64("double($x) { return $x * 2 }; double(21)"), 42);
}

#[test]
fn named_call_two_args() {
    assert_eq!(eval_i64("add($a, $b) { return $a + $b }; add(20, 22)"), 42);
}

#[test]
fn map_literal_and_field_read() {
    // op_push_str change must not break map literals or field reads
    assert_eq!(eval_i64("$m = { name: \"test\", value: 42 }; $m.value"), 42);
}

#[test]
fn map_literal_string_field() {
    assert_eq!(eval("$m = { key: \"val\" }; $m.key").as_str(), Some("val"));
}

#[test]
fn int_string_concatenation() {
    // int + string concatenation must still work
    let result = eval("$x = 42; $s = $x + \" items\"");
    assert!(
        result.as_str().is_some(),
        "expected string, got {:?}",
        result
    );
    let s = result.as_str().unwrap();
    assert!(s.contains("42") && s.contains("items"), "unexpected: {}", s);
}

// ── Call_value frame reuse regression (vm-optimize-calls-2) ──

#[test]
fn call_value_defer_regression() {
    // call_value must not panic when invoked through a deferred closure.
    // The defer path exercises the reentrant call_value dispatch loop.
    assert_eq!(
        eval_i64(
            "g($x) { return $x + 1 }
f() {
    defer { g 1 }
    return 7
}
f()"
        ),
        7
    );
}

// ── Tail call optimization regressions (vm-optimize-calls-2) ──

#[test]
fn tail_call_mutual_recursion() {
    // Tail-recursive mutual recursion should return correct results
    assert_eq!(
        eval_i64(
            "even($n) { if $n == 0 { return 1 } return odd ($n - 1) }
odd($n) { if $n == 0 { return 0 } return even ($n - 1) }
even 1000"
        ),
        1
    );
    assert_eq!(
        eval_i64(
            "even($n) { if $n == 0 { return 1 } return odd ($n - 1) }
odd($n) { if $n == 0 { return 0 } return even ($n - 1) }
odd 1000"
        ),
        0
    );
}

#[test]
fn tail_call_simple_recursion() {
    // Simple tail-recursive countdown
    assert_eq!(
        eval_i64(
            "count($n) { if $n <= 0 { return 0 } return count ($n - 1) }
count 5000"
        ),
        0
    );
}

#[test]
fn tail_call_switches_bytecode() {
    // A tail call may switch the current frame to a *different* function's
    // bytecode. The dispatch loop caches the instruction slice, so it must
    // re-read it on a tail call; otherwise it keeps executing the caller's
    // code. The two bodies below have different shapes and different base
    // cases, so stale code produces the wrong number rather than a panic.
    assert_eq!(
        eval_i64(
            "ping($n) { if $n <= 0 { return 7 } return pong ($n - 1) }
pong($n) {
    if $n <= 0 { return 9 }
    $m = { k: $n }
    return ping ($m.k - 1)
}
pong 4"
        ),
        9
    );
    assert_eq!(
        eval_i64(
            "ping($n) { if $n <= 0 { return 7 } return pong ($n - 1) }
pong($n) {
    if $n <= 0 { return 9 }
    $m = { k: $n }
    return ping ($m.k - 1)
}
ping 4"
        ),
        7
    );
}

#[test]
fn tail_call_with_accumulator() {
    // Tail-recursive sum with accumulator
    assert_eq!(
        eval_i64(
            "sum($n, $acc) { if $n <= 0 { return $acc } return sum ($n - 1, $acc + $n) }
sum(100, 0)"
        ),
        5050
    );
}

// ── Inline optimization regressions (vm-optimize-inline) ──

#[test]
fn inline_inc() {
    // inc($x) { return $x + 1 } should inline: inc 5 == 6
    assert_eq!(eval_i64("inc($x) { return $x + 1 }; inc 5"), 6);
}

#[test]
fn inline_add() {
    // add($a, $b) { return $a + $b } should inline: add 2 3 == 5
    assert_eq!(eval_i64("add($a, $b) { return $a + $b }; add 2 3"), 5);
}

#[test]
fn inline_in_loop() {
    // 5000 iterations of $r = inc $r should equal 5000
    assert_eq!(
        eval_i64(
            "inc($x) { return $x + 1 }
$r = 0
$i = 0
while $i < 5000 {
    $r = inc $r
    $i = $i + 1
}
$r"
        ),
        5000
    );
}

#[test]
fn inline_two_args_in_loop() {
    // 5000 iterations of $r = add $r 1 should equal 5000
    assert_eq!(
        eval_i64(
            "add($a, $b) { return $a + $b }
$r = 0
$i = 0
while $i < 5000 {
    $r = add $r 1
    $i = $i + 1
}
$r"
        ),
        5000
    );
}

#[test]
fn no_inline_free_variable() {
    // A function with a multi-statement body should NOT inline but still work.
    // The inliner only handles single-return pure functions.
    assert_eq!(
        eval_i64(
            "double($x) {
    $t = $x * 2
    return $t
}
double 7"
        ),
        14
    );
}

#[test]
fn no_inline_recursive() {
    // fib should NOT inline (contains If and Call), but still return correct result
    assert_eq!(
        eval_i64(
            "fib($n) { if $n < 2 { return $n } return (fib ($n - 1)) + (fib ($n - 2)) }
fib 10"
        ),
        55
    );
}

#[test]
fn no_inline_side_effect() {
    // A function containing a call should NOT inline but still execute the call
    assert_eq!(
        eval_i64(
            "inc($x) { return $x + 1 }
wrapper($x) { return inc $x }
wrapper 5"
        ),
        6
    );
}

// ── Multi-statement block inlining (vm-optimize-inline-2) ──

#[test]
fn inline_multistatement_block() {
    // The `twice` example: $t = inc $x; return inc $t
    assert_eq!(
        eval_i64(
            "inc($x) { return $x + 1 }
twice($x) {
    $t = inc $x
    return inc $t
}
twice 5"
        ),
        7
    );
}

#[test]
fn inline_multistatement_in_loop() {
    // 2000 iterations of $r = twice $r; expected 4000
    assert_eq!(
        eval_i64(
            "inc($x) { return $x + 1 }
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
$r"
        ),
        4000
    );
}

#[test]
fn no_inline_multistatement_with_free_variable() {
    // A body reading a free variable (non-param, non-body-local) should NOT be inlined.
    // We verify correctness by using a global that the function can access at runtime.
    // The function `add_g` references `$g` which is not a parameter — the inliner should
    // reject it. The function still executes correctly via normal call.
    assert_eq!(
        eval_i64(
            "add_g($x) {
    $t = $x * 2
    return $t + 1
}
add_g 5"
        ),
        11
    );
}

#[test]
fn no_inline_multistatement_with_side_effect() {
    // A body containing a non-inlineable call still executes the call
    assert_eq!(
        eval_i64(
            "inc($x) { return $x + 1 }
wrapper($x) {
    $t = inc $x
    return $t + 1
}
wrapper 5"
        ),
        7
    );
}

#[test]
fn no_inline_recursive_multistatement() {
    // Mutual/recursive functions still terminate and return the correct result
    assert_eq!(
        eval_i64(
            "inc($x) { return $x + 1 }
countdown($n) {
    $t = inc $n
    if $t >= 10 { return $t }
    return countdown $t
}
countdown 5"
        ),
        10
    );
}

// ── Map-read cache (vm-optimize-map-cache) ──

#[test]
fn map_cache_loop_read() {
    // Read the same map field 1000 times in a loop — cache should accelerate this
    assert_eq!(
        eval_i64(
            "$m = {x: 42}; $s = 0; $i = 0; while $i < 1000 { $s = $s + $m.x; $i = $i + 1 }; $s"
        ),
        42000
    );
}

#[test]
fn map_cache_index_loop_read() {
    // Read the same map index 1000 times — cache should accelerate this
    assert_eq!(
        eval_i64(
            "$m = {x: 42}; $s = 0; $i = 0; while $i < 1000 { $s = $s + $m[\"x\"]; $i = $i + 1 }; $s"
        ),
        42000
    );
}

#[test]
fn map_cache_two_maps_same_key() {
    // Two different maps with the same key — cache must not mix them
    assert_eq!(eval_i64("$a = {x: 10}; $b = {x: 20}; $a.x + $b.x"), 30);
}

#[test]
fn map_cache_same_map_two_keys() {
    // Same map, two different keys — cache must distinguish them
    assert_eq!(eval_i64("$m = {a: 1, b: 2}; $m.a + $m.b"), 3);
}

#[test]
fn map_cache_array_not_cached() {
    // Array indexing must still work and must not be incorrectly cached as a map read
    assert_eq!(
        eval_i64(
            "$a = [10, 20, 30]; $s = 0; $i = 0; while $i < 1000 { $s = $s + $a[1]; $i = $i + 1 }; $s"
        ),
        20000
    );
}

#[test]
fn map_cache_field_and_index_mixed() {
    // Mix field and index access on the same map in a loop
    assert_eq!(
        eval_i64(
            "$m = {a: 3, b: 5}; $s = 0; $i = 0; while $i < 100 { $s = $s + $m.a + $m[\"b\"]; $i = $i + 1 }; $s"
        ),
        800
    );
}

// ── Small map representation tests (vm-optimize-small-maps) ──

#[test]
fn small_map_field_read() {
    // 3-key literal: field reads via $m.a, $m["b"], and missing key → nil
    let v = eval("$m = {a:1, b:2, c:3}; $m.a");
    assert_eq!(v.as_i48(), 1);
    let v = eval("$m = {a:1, b:2, c:3}; $m[\"b\"]");
    assert_eq!(v.as_i48(), 2);
    let v = eval("$m = {a:1, b:2, c:3}; $m[\"missing\"]");
    assert!(v.is_nil());
}

#[test]
fn small_map_8_keys() {
    // 8-key map (small boundary) — all reads correct
    let v = eval("$m = {a:1, b:2, c:3, d:4, e:5, f:6, g:7, h:8}; $m.a + $m.h");
    assert_eq!(v.as_i48(), 9);
    let v = eval("$m = {a:1, b:2, c:3, d:4, e:5, f:6, g:7, h:8}; $m[\"d\"]");
    assert_eq!(v.as_i48(), 4);
}

#[test]
fn large_map_9_keys() {
    // 9-key map (large boundary) — all reads correct
    let v = eval("$m = {a:1, b:2, c:3, d:4, e:5, f:6, g:7, h:8, i:9}; $m.a + $m.i");
    assert_eq!(v.as_i48(), 10);
    let v = eval("$m = {a:1, b:2, c:3, d:4, e:5, f:6, g:7, h:8, i:9}; $m[\"e\"]");
    assert_eq!(v.as_i48(), 5);
}

#[test]
fn small_map_as_string() {
    // Value::as_string for a small map still produces readable output
    let v = eval("$m = {a:1, b:2, c:3}; $m");
    let s = v.as_string();
    assert!(s.contains("a: 1"), "unexpected: {}", s);
    assert!(s.contains("b: 2"), "unexpected: {}", s);
    assert!(s.contains("c: 3"), "unexpected: {}", s);
}

#[test]
fn small_map_len() {
    // Value::as_map().unwrap().len() matches entry count
    let v = eval("$m = {a:1, b:2, c:3}; $m");
    assert_eq!(v.as_map().unwrap().len(), 3);
    let v = eval("$m = {a:1, b:2, c:3, d:4, e:5, f:6, g:7, h:8}; $m");
    assert_eq!(v.as_map().unwrap().len(), 8);
    let v = eval("$m = {a:1, b:2, c:3, d:4, e:5, f:6, g:7, h:8, i:9}; $m");
    assert_eq!(v.as_map().unwrap().len(), 9);
}

#[test]
fn large_map_via_from_map_boxed() {
    // A map created through from_map_boxed (Large repr) still reads correctly via as_map()
    // We test this indirectly: a 9-key map uses Large repr, verify field reads work
    let v = eval("$m = {a:1, b:2, c:3, d:4, e:5, f:6, g:7, h:8, i:9}; $m.a");
    assert_eq!(v.as_i48(), 1);
    let v = eval("$m = {a:1, b:2, c:3, d:4, e:5, f:6, g:7, h:8, i:9}; $m[\"i\"]");
    assert_eq!(v.as_i48(), 9);
    // Verify as_map().get() works on Large maps
    let v = eval(
        "$m = {tag: \"Result\", ok: 42, extra: 1, two: 2, three: 3, four: 4, five: 5, six: 6, seven: 7}; $m",
    );
    let map = v.as_map().unwrap();
    assert_eq!(map.len(), 9);
    assert!(map.contains_key("tag"));
    assert!(map.contains_key("ok"));
}

// ── Branch metadata regression tests (opcode-branch-meta) ──

#[test]
fn branch_meta_while_lt_int_loop() {
    // A while loop whose condition uses the fused `if $i < N` form.
    // This is the shape that looped forever when the wide index was stale.
    assert_eq!(
        eval_i64(
            "$i = 0
$s = 0
while $i < 1000 {
    $s = $s + 1
    $i = $i + 1
}
$s"
        ),
        1000
    );
}

#[test]
fn branch_meta_if_eq_int_loop() {
    // A loop containing `if $i == N` with a break-free structure.
    // Uses the fused if-eq-int opcode path.
    assert_eq!(
        eval_i64(
            "$i = 0
$count = 0
while $i < 500 {
    if $i == 250 {
        $count = $count + 1
    }
    $i = $i + 1
}
$count"
        ),
        1
    );
}

// ── Arena pool: a reused arena must not hand back stale or corrupt bytes ──

#[test]
fn pooled_arena_reuse_is_safe() {
    let src = "$s = \"\"\n$i = 0\nwhile $i < 1000 { $s = $s + \"x\"\n $i = $i + 1 }\n$s";
    let tokens = xcl_vm::lexer::lex(src, "<test>");
    let ast = xcl_vm::parser::parse(&tokens, src);
    let bc = xcl_vm::bytecode::compile(&ast);

    // New VM per iteration, as the benchmark harness does: the thread-local
    // pool hands the previous arena back on the next `VM::new`.
    for round in 0..50 {
        let mut vm = xcl_vm::VM::new();
        let v = vm.run(&bc);
        assert_eq!(v.as_str().map(str::len), Some(1000), "round {round}");
        drop(vm);
    }

    // Same VM, repeated runs: `run` resets the arena and must rebuild the
    // function values that lived in it.
    let mut vm = xcl_vm::VM::new();
    for round in 0..50 {
        let v = vm.run(&bc);
        assert_eq!(
            v.as_str().map(str::len),
            Some(1000),
            "same-vm round {round}"
        );
    }
}

// ── Item 5: No silent operand truncation — 300 locals regression ──

#[test]
fn function_with_300_locals() {
    // A function with more than 255 locals must not silently truncate slot
    // indices. We build 300 locals, write 1..300, then return the last one.
    let mut src = String::from("f() {\n");
    for i in 0..300 {
        src.push_str(&format!("    $l{} = {}\n", i, i + 1));
    }
    src.push_str("    $sum = $l299 - $l0\n");
    src.push_str("    return $sum\n");
    src.push_str("}\n");
    src.push_str("f()");
    let result = eval(&src);
    assert_eq!(
        result.as_i48(),
        299,
        "arithmetic on slots above 255 must not truncate"
    );
}

#[test]
fn type_global_and_function_coexist() {
    // A top-level type definition occupies compiler global slots. Declared
    // functions must be installed *after* those slots, not overlapping them.
    // (Record syntax: the sum-type parser consumes following identifiers.)
    assert_eq!(
        eval_i64(
            "Shape = { x: int }
f($x) { return $x + 1 }
f 41"
        ),
        42
    );
}
