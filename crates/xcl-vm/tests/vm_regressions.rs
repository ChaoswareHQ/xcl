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
