use super::*;
use crate::bytecode::compile;
use crate::compiler::lexer::lex;
use crate::compiler::parser::parse;

fn eval_with_args(src: &str, name: &str, args: &[&str]) -> Value {
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    let mut vm = VM::new();
    let vals: Vec<Value> = args
        .iter()
        .map(|s| Value::from_str_boxed((*s).to_string()))
        .collect();
    vm.set_script_args(name, vals);
    let result = vm.run(&bc);
    // Tests may inspect heap values (strings/maps) backed by the VM arena.
    // Keep the arena alive for the duration of the test process.
    core::mem::forget(vm);
    result
}

#[test]
fn test_arg_count() {
    let result = eval_with_args("$#", "script.xcl", &["a", "b", "c"]);
    assert_eq!(result.as_i48(), 3);
}

#[test]
fn test_positional_arg() {
    let result = eval_with_args("$1", "script.xcl", &["a"]);
    assert!(
        result.is_ptr(),
        "expected pointer, got tag={:016X}",
        result.0
    );
    assert_eq!(result.as_str(), Some("a"));
}

#[test]
fn test_positional_arg_out_of_range() {
    let result = eval_with_args("$10", "script.xcl", &["a"]);
    assert!(result.is_nil());
}

#[test]
fn test_script_name() {
    let result = eval_with_args("$0", "script.xcl", &[]);
    assert_eq!(result.as_str(), Some("script.xcl"));
}

#[test]
fn test_args_array() {
    // Keep the VM alive: `$@` allocates its array in the VM arena.
    let src = "$@";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    let mut vm = VM::new();
    let vals: Vec<Value> = ["a", "b"]
        .iter()
        .map(|s| Value::from_str_boxed((*s).to_string()))
        .collect();
    vm.set_script_args("script.xcl", vals);
    let result = vm.run(&bc);
    let arr = result.as_array().expect("$@ should return an array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].as_str(), Some("a"));
    assert_eq!(arr[1].as_str(), Some("b"));
}

#[test]
fn test_args_joined() {
    let result = eval_with_args("$*", "script.xcl", &["a", "b", "c d"]);
    assert_eq!(result.as_str(), Some("a b c d"));
}

#[test]
#[should_panic(expected = "not supported")]
fn test_unsupported_dollar_dollar() {
    eval_with_args("$$", "script.xcl", &[]);
}

#[test]
#[should_panic(expected = "not supported")]
fn test_unsupported_dollar_question() {
    eval_with_args("$?", "script.xcl", &[]);
}

#[test]
#[should_panic(expected = "not supported")]
fn test_unsupported_dollar_bang() {
    eval_with_args("$!", "script.xcl", &[]);
}

#[test]
#[should_panic(expected = "not supported")]
fn test_unsupported_dollar_dash() {
    eval_with_args("$-", "script.xcl", &[]);
}

#[test]
#[should_panic(expected = "read-only")]
fn test_assignment_read_only() {
    eval_with_args("$1 = 5", "script.xcl", &[]);
}

#[test]
fn test_special_var_as_bare_call_arg() {
    let src = r#"
        id($x) { return $x }
        id $1
    "#;
    let result = eval_with_args(src, "script.xcl", &["hello"]);
    assert_eq!(result.as_str(), Some("hello"));
}

#[test]
fn test_special_vars_in_function_scope() {
    let src = r#"
        f($a, $b) {
            if $# == 3 { return $3 }
            return "bad"
        }
        f "x" "y" "z"
    "#;
    let result = eval_with_args(src, "script.xcl", &[]);
    assert_eq!(result.as_str(), Some("z"));
}

#[test]
fn test_script_name_inside_function() {
    let src = r#"
        f() { return $0 }
        f
    "#;
    let result = eval_with_args(src, "script.xcl", &[]);
    assert_eq!(result.as_str(), Some("script.xcl"));
}

#[test]
fn test_bare_calls_do_not_merge_across_newlines() {
    let src = "f \"a\"\nf \"b\"\n";
    let tokens = crate::compiler::lexer::lex(src, "<test>");
    let ast = crate::compiler::parser::parse(&tokens, src);
    assert_eq!(ast.len(), 2, "expected two statements, got {:?}", ast);
}

#[test]
fn test_nested_call_argument_does_not_swallow_next_line() {
    let src = r#"
        f($x) { return $x }
        g($x) { return $x }
        h($x, $y) { return $y }
        h (f "a") (g "b")
        f "c"
    "#;
    let tokens = crate::compiler::lexer::lex(src, "<test>");
    let ast = crate::compiler::parser::parse(&tokens, src);
    // 3 function definitions + 2 call statements
    assert_eq!(ast.len(), 5, "unexpected statement count: {:?}", ast);
}

// ── Bitwise OR/XOR/NOT, exponentiation, ternary tests ──

#[test]
fn test_bitor() {
    let result = eval_with_args("1 | 2", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 3);
}

#[test]
fn test_bitxor() {
    let result = eval_with_args("6 ^ 3", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 5);
}

#[test]
fn test_bitnot() {
    let result = eval_with_args("~0", "script.xcl", &[]);
    assert_eq!(result.as_i48(), -1);
}

#[test]
fn test_pow_basic() {
    let result = eval_with_args("2 ** 10", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 1024);
}

#[test]
fn test_pow_negative_exp() {
    let result = eval_with_args("2 ** -1", "script.xcl", &[]);
    assert!((result.as_f64() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn test_pow_right_associative() {
    // 2 ** 3 ** 2 = 2 ** 9 = 512
    let result = eval_with_args("2 ** 3 ** 2", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 512);
}

#[test]
fn test_pow_mixed_float_operands() {
    let result = eval_with_args("2.0 ** 3", "script.xcl", &[]);
    assert!(result.is_float());
    assert!((result.as_f64() - 8.0).abs() < f64::EPSILON);

    let result = eval_with_args("2 ** 3.0", "script.xcl", &[]);
    assert!(result.is_float());
    assert!((result.as_f64() - 8.0).abs() < f64::EPSILON);
}

#[test]
fn test_pow_i48_boundary() {
    // 2^40 fits in the 48-bit signed payload.
    let result = eval_with_args("2 ** 40", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 1_099_511_627_776);

    // 2^47 does not fit; it must fall back to float.
    let result = eval_with_args("2 ** 47", "script.xcl", &[]);
    assert!(result.is_float());
    assert!((result.as_f64() - 140_737_488_355_328.0).abs() < 1.0);
}

#[test]
fn test_precedence_bitor_bitand() {
    // 1 | 2 & 3 → 1 | (2 & 3) → 1 | 2 → 3
    let result = eval_with_args("1 | 2 & 3", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 3);
}

#[test]
fn test_precedence_bitxor_bitand() {
    // 1 ^ 2 & 3 → 1 ^ (2 & 3) → 1 ^ 2 → 3
    let result = eval_with_args("1 ^ 2 & 3", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 3);
}

#[test]
fn test_precedence_bitor_bitxor() {
    // 1 | 2 ^ 3 → (1 | 2) ^ 3 → 3 ^ 3 → 0... wait, task says 1
    // Actually: | binds looser than ^, so 1 | 2 ^ 3 → 1 | (2 ^ 3) → 1 | 1 → 1
    let result = eval_with_args("1 | 2 ^ 3", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 1);
}

#[test]
fn test_compound_bitor() {
    let result = eval_with_args("$x = 5; $x |= 3; $x", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 7);
}

#[test]
fn test_compound_bitxor() {
    let result = eval_with_args("$x = 5; $x ^= 3; $x", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 6);
}

#[test]
fn test_compound_pow() {
    let result = eval_with_args("$x = 3; $x **= 2; $x", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 9);
}

#[test]
fn test_conditional_true() {
    let result = eval_with_args("1 if true else 2", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 1);
}

#[test]
fn test_conditional_false() {
    let result = eval_with_args("1 if false else 2", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 2);
}

#[test]
fn test_conditional_right_associative() {
    let result = eval_with_args("1 if false else 2 if true else 3", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 2);
}

#[test]
fn test_conditional_string_example() {
    let result = eval_with_args(
        r#"$num = 6; $label = "WEEKEND!" if $num > 5 else "Workday"; $label"#,
        "script.xcl",
        &[],
    );
    assert_eq!(result.as_str(), Some("WEEKEND!"));
}

#[test]
fn test_if_statement_not_swallowed_by_conditional() {
    // The newline before `if` must keep it as a statement, not a ternary.
    let src = "$x = 1\nif true { $x = 2 }\n$x";
    let result = eval_with_args(src, "script.xcl", &[]);
    assert_eq!(result.as_i48(), 2);
}

#[test]
fn test_lambda_body_bitor() {
    // $f = |$x| $x | 1; $f 2 → 3
    let result = eval_with_args("$f = |$x| $x | 1; $f 2", "script.xcl", &[]);
    assert_eq!(result.as_i48(), 3);
}

#[test]
fn test_logical_or_still_works() {
    let result = eval_with_args("false || true", "script.xcl", &[]);
    assert!(result.as_bool());
}

#[test]
fn test_sum_type_parsing() {
    // Shape = A | B should produce a TypeDef, not a binary expression
    let src = "Shape = A | B";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    assert_eq!(ast.len(), 1);
    match &ast[0] {
        crate::compiler::ast::AstNode::TypeDef { name, .. } => {
            assert_eq!(name, "Shape");
        }
        other => panic!("Expected TypeDef, got {:?}", other),
    }
}

#[test]
fn test_while_loop_sum() {
    let result = eval_with_args(
        "$s = 0; $i = 0; while $i < 5 { $s = $s + $i; $i = $i + 1 }; $s",
        "script.xcl",
        &[],
    );
    assert_eq!(result.as_i48(), 10);
}

#[test]
fn test_for_number_loop() {
    let result = eval_with_args(
        "$s = 0; for $i in 0..10 { $s = $s + $i }; $s",
        "script.xcl",
        &[],
    );
    assert_eq!(result.as_i48(), 45);
}

#[test]
fn test_named_call_in_while() {
    let result = eval_with_args(
        "inc($x) { return $x + 1 }; $i = 0; $r = 0; while $i < 5 { $r = inc $r; $i = $i + 1 }; $r",
        "script.xcl",
        &[],
    );
    assert_eq!(result.as_i48(), 5);
}

#[test]
fn test_parenthesized_lambda_arg() {
    // f (|$x| $x) should parse as a call with a lambda argument
    let src = "f (|$x| $x)";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    assert_eq!(ast.len(), 1);
    match &ast[0] {
        crate::compiler::ast::AstNode::Call { callee, args, .. } => {
            match callee.as_ref() {
                crate::compiler::ast::AstNode::Ident { name, .. } => {
                    assert_eq!(name, "f");
                }
                other => panic!("Expected Ident callee, got {:?}", other),
            }
            assert_eq!(args.len(), 1);
            match &args[0].value {
                crate::compiler::ast::AstNode::Lambda { .. } => {}
                other => panic!("Expected Lambda arg, got {:?}", other),
            }
        }
        other => panic!("Expected Call, got {:?}", other),
    }
}

// ── String concat fast path tests ──

#[test]
fn test_string_concat_two_strings() {
    let result = eval_with_args(r#""hello" + "world""#, "script.xcl", &[]);
    assert_eq!(result.as_str(), Some("helloworld"));
}

#[test]
fn test_string_concat_int_and_string() {
    let result = eval_with_args(r#"1 + "x""#, "script.xcl", &[]);
    assert_eq!(result.as_str(), Some("1x"));
}

#[test]
fn test_string_concat_string_and_int() {
    let result = eval_with_args(r#""x" + 1"#, "script.xcl", &[]);
    assert_eq!(result.as_str(), Some("x1"));
}

#[test]
fn test_string_concat_loop() {
    // Concatenate in a loop to exercise the fast path repeatedly
    let result = eval_with_args(
        "$s = \"\"; for $i in 0..100 { $s = $s + \"a\" }; $s",
        "script.xcl",
        &[],
    );
    let s = result.as_str().expect("concat loop should return string");
    assert_eq!(s.len(), 100);
    assert!(s.chars().all(|c| c == 'a'));
}

// ── Map lookup fast path tests ──

#[test]
fn test_map_field_read() {
    let result = eval_with_args(
        r#"$m = {name: "Alice", age: 30}; $m.name"#,
        "script.xcl",
        &[],
    );
    assert_eq!(result.as_str(), Some("Alice"));
}

#[test]
fn test_map_index_read() {
    let result = eval_with_args(
        r#"$m = {name: "Bob", age: 25}; $m["name"]"#,
        "script.xcl",
        &[],
    );
    assert_eq!(result.as_str(), Some("Bob"));
}

#[test]
fn test_map_field_read_loop() {
    // Read a field in a loop to exercise the fast path
    let result = eval_with_args(
        r#"$m = {x: 42}; $s = 0; for $i in 0..1000 { $s = $s + $m.x }; $s"#,
        "script.xcl",
        &[],
    );
    assert_eq!(result.as_i48(), 42000);
}

#[test]
fn test_map_create_and_read() {
    let result = eval_with_args(
        r#"$m = {a: 1, b: 2, c: 3}; $m.a + $m.b + $m.c"#,
        "script.xcl",
        &[],
    );
    assert_eq!(result.as_i48(), 6);
}

// ── String representation tests (arena bytes, cached hash) ──

#[test]
fn test_string_equality_across_constructions() {
    // Strings built via from_str, from_str_boxed, and concatenation
    // with the same bytes should compare equal.
    let boxed = Value::from_str_boxed("hello".to_string());
    let arena = bumpalo::Bump::new();
    let arena_str = Value::from_str("hello".to_string(), &arena);
    assert_eq!(boxed.as_str(), Some("hello"));
    assert_eq!(arena_str.as_str(), Some("hello"));
    // Equality via Value::eq_
    assert!(Value::eq_(boxed, boxed));
    assert!(Value::eq_(arena_str, arena_str));
    // Cross-construction: same bytes, different pointers
    assert_eq!(boxed.as_str(), arena_str.as_str());
    core::mem::forget(arena);
}

#[test]
fn test_str_hash_consistency() {
    let boxed = Value::from_str_boxed("test-key".to_string());
    let arena = bumpalo::Bump::new();
    let arena_str = Value::from_str("test-key".to_string(), &arena);

    // Both should have the same cached hash
    let h1 = boxed.str_hash().expect("boxed string should have hash");
    let h2 = arena_str.str_hash().expect("arena string should have hash");
    assert_eq!(h1, h2, "same bytes must produce same cached hash");

    // Non-string values return None
    assert!(Value::from_i48(42).str_hash().is_none());
    assert!(Value::nil().str_hash().is_none());
    assert!(Value::bool(true).str_hash().is_none());
    core::mem::forget(arena);
}

#[test]
fn test_concatenation_produces_correct_value() {
    let result = eval_with_args(r#""hello" + " world""#, "script.xcl", &[]);
    assert_eq!(result.as_str(), Some("hello world"));
}

#[test]
fn test_concatenation_loop_correctness() {
    // 3-part concatenation in a loop stays correct
    let result = eval_with_args(
        r#"$s = ""; for $i in 0..50 { $s = $s + "a" + "b" }; $s"#,
        "script.xcl",
        &[],
    );
    let s = result.as_str().expect("concat loop should return string");
    assert_eq!(s.len(), 100);
    for c in s.chars() {
        assert!(c == 'a' || c == 'b');
    }
}

#[test]
fn test_map_with_runtime_key() {
    // A map keyed by a string built at runtime exercises the cached-hash
    // path with a key that is not the compiler's interned constant.
    // We build the key via concatenation, then use it for lookup.
    let result = eval_with_args(
        r#"$k = "x" + "y"; $m = {xy: 42}; $m[$k]"#,
        "script.xcl",
        &[],
    );
    assert_eq!(result.as_i48(), 42);
}

// ── Item 2 + Item 6: Memory-stability test ──

#[test]
fn test_memory_stability_500_runs() {
    // Run the same compiled program 500 times on one VM and assert that
    // globals.len(), global_index.len(), and frames.len() after run 500
    // equal their values after run 2.
    let src = r#"
        f($n) {
            if $n <= 0 { return 0 }
            return f ($n - 1) + 1
        }
        $a = [1, 2, 3]
        $a = $a + [4]
        f 10
    "#;
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    let mut vm = VM::new();

    // Sample the state after the second run on the *same* VM. Anything that
    // grows per run (globals, function registrations, frames) shows up here.
    let mut after_second: Option<(usize, usize, usize)> = None;
    for run in 1..=500 {
        let _ = vm.run(&bc);
        if run == 2 {
            after_second = Some((vm.globals.len(), vm.global_index.len(), vm.frames.len()));
        }
    }
    let (globals_2, global_index_2, frames_2) = after_second.expect("two runs");

    assert_eq!(
        vm.globals.len(),
        globals_2,
        "globals.len() after 500 runs ({}) must equal after 2 runs ({})",
        vm.globals.len(),
        globals_2
    );
    assert_eq!(
        vm.global_index.len(),
        global_index_2,
        "global_index.len() after 500 runs ({}) must equal after 2 runs ({})",
        vm.global_index.len(),
        global_index_2
    );
    assert_eq!(
        vm.frames.len(),
        frames_2,
        "frames.len() after 500 runs ({}) must equal after 2 runs ({})",
        vm.frames.len(),
        frames_2
    );
    assert!(
        vm.context_stack.is_empty(),
        "context_stack must be empty after repeated runs"
    );
}

// ── Tier 2: frame code pointer tests ──

#[test]
fn frame_code_pointer_survives_repeated_runs() {
    // Compile three different programs and run them round-robin 50 times
    // each on one VM to catch stale pointer issues.
    let src_recursion = "f($n) { if $n <= 0 { return 1 } return f ($n - 1) * 2 }; f 10";
    let src_closure = "$f = |$x| $x * 3; $f 7";
    let src_tail = "ping($n) { if $n <= 0 { return 7 } return pong ($n - 1) }; pong($n) { if $n <= 0 { return 9 } return ping ($n - 1) }; ping 5";

    let tokens1 = lex(src_recursion, "<test>");
    let ast1 = parse(&tokens1, src_recursion);
    let bc1 = compile(&ast1);

    let tokens2 = lex(src_closure, "<test>");
    let ast2 = parse(&tokens2, src_closure);
    let bc2 = compile(&ast2);

    let tokens3 = lex(src_tail, "<test>");
    let ast3 = parse(&tokens3, src_tail);
    let bc3 = compile(&ast3);

    let mut vm = VM::new();
    for _ in 0..50 {
        assert_eq!(vm.run(&bc1).as_i48(), 1024, "recursion failed");
        assert_eq!(vm.run(&bc2).as_i48(), 21, "closure failed");
        assert_eq!(vm.run(&bc3).as_i48(), 9, "tail call failed");
    }
}

#[test]
fn tail_call_across_bytecodes_after_run() {
    // Run a tail-call program, then a different program, then the tail-call
    // program again on the same VM.
    let src_tail = "ping($n) { if $n <= 0 { return 7 } return pong ($n - 1) }; pong($n) { if $n <= 0 { return 9 } return ping ($n - 1) }; ping 4";
    let src_other = "$x = 100; $x + 1";

    let tokens1 = lex(src_tail, "<test>");
    let ast1 = parse(&tokens1, src_tail);
    let bc1 = compile(&ast1);

    let tokens2 = lex(src_other, "<test>");
    let ast2 = parse(&tokens2, src_other);
    let bc2 = compile(&ast2);

    let mut vm = VM::new();
    assert_eq!(vm.run(&bc1).as_i48(), 7, "first tail call");
    assert_eq!(vm.run(&bc2).as_i48(), 101, "other program");
    assert_eq!(vm.run(&bc1).as_i48(), 7, "tail call again");
}

#[test]
fn call_value_after_run() {
    // Keep a function Value returned by run, call it via call_value after
    // the VM has run a different program.
    let src1 = "mk($n) { return |$x| $x + $n }; mk 10";
    let src2 = "42 * 2";

    let tokens1 = lex(src1, "<test>");
    let ast1 = parse(&tokens1, src1);
    let bc1 = compile(&ast1);

    let tokens2 = lex(src2, "<test>");
    let ast2 = parse(&tokens2, src2);
    let bc2 = compile(&ast2);

    let mut vm = VM::new();
    let func = vm.run(&bc1);
    assert!(func.as_func().is_some(), "expected function from first run");

    // Run a different program
    assert_eq!(vm.run(&bc2).as_i48(), 84, "different program");

    // Call the saved function via call_value
    let result = vm.call_value(func, &[Value::from_i48(5)]);
    assert_eq!(result.as_i48(), 15, "call_value after run");
}

#[test]
fn vm_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<VM>();
}

// ── JIT (feature `jit`) ──

#[cfg(feature = "jit")]
#[test]
fn jit_compiles_integer_loops_and_matches_an_oracle() {
    // Counting loop: the JIT must produce the same value as the oracle.
    // `$s` starts from the argument count so the counted loop is not folded
    // away before the JIT sees it.
    let src = "$s = $#\n$i = 0\nwhile $i < 1000 {\n    $s = $s + $i\n    $i = $i + 1\n}\n$s";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    assert!(prepare_jit(&bc).is_some(), "counting loop should compile");
    let mut vm = VM::new();
    assert_eq!(vm.run(&bc).as_i48(), 499_500);

    // Mock LCG: compiler output checked against a Rust oracle.
    let src = "$x = 42\n$i = 0\nwhile $i < 100000 {\n    $x = ($x * 37 + 12345) % 1000003\n    $i = $i + 1\n}\n$x";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    assert!(prepare_jit(&bc).is_some(), "lcg loop should compile");
    let mut expect = 42i64;
    for _ in 0..100_000 {
        expect = (expect * 37 + 12_345) % 1_000_003;
    }
    let mut vm = VM::new();
    assert_eq!(vm.run(&bc).as_i48(), expect);

    // Bitwise loop: shifts, or, and.
    let src = "$x = 12345\n$i = 0\nwhile $i < 200000 {\n    $x = (($x << 5) | ($x >> 3)) & 16777215\n    $x = ($x + $i) & 16777215\n    $i = $i + 1\n}\n$x";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    assert!(prepare_jit(&bc).is_some(), "bitwise loop should compile");
    let mut x = 12345i64;
    let mut i = 0i64;
    while i < 200_000 {
        x = ((x << 5) | (x >> 3)) & 16_777_215;
        x = (x + i) & 16_777_215;
        i += 1;
    }
    let mut vm = VM::new();
    assert_eq!(vm.run(&bc).as_i48(), x);
}

#[cfg(feature = "jit")]
#[test]
fn jit_compiles_every_loop_in_a_unit() {
    let src = "$a = $#\n$i = 0\nwhile $i < 100 {\n    $a = $a + $i\n    $i = $i + 1\n}\n$b = $#\n$j = 0\nwhile $j < 100 {\n    $b = $b + 1\n    $j = $j + 1\n}\n$a + $b";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    let unit = prepare_jit(&bc).expect("both loops compile");
    assert_eq!(unit.entries.len(), 2, "expected two compiled loops");
    assert_eq!(
        unit.shadow.iter().filter(|&&s| s != 0).count(),
        2,
        "both headers must be in the shadow table"
    );
    let mut vm = VM::new();
    assert_eq!(vm.run(&bc).as_i48(), 4950 + 100);
}

#[cfg(feature = "jit")]
#[test]
fn jit_compiles_array_index_loops() {
    let src = "$a = [10,20,30,40,50]\n$i = 0\n$s = 0\nwhile $i < 5 {\n    $s = $s + $a[$i]\n    $i = $i + 1\n}\n$s";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    let unit = prepare_jit(&bc).expect("array loop compiles");
    assert_eq!(unit.entries.len(), 1);
    assert_eq!(unit.entries[0].access_len, 1, "one array access site");
    let mut vm = VM::new();
    assert_eq!(vm.run(&bc).as_i48(), 150);

    // Negative indices are normalised exactly like the interpreter.
    let src = "$a = [10,20,30]\n$i = -3\n$s = 0\nwhile $i < 0 {\n    $s = $s + $a[$i]\n    $i = $i + 1\n}\n$s";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    let mut vm = VM::new();
    assert_eq!(vm.run(&bc).as_i48(), 60);
}

#[cfg(feature = "jit")]
#[test]
fn jit_falls_back_when_the_container_is_not_an_array() {
    // The loop is structurally compilable, but the local is a map. The
    // cache fill must fail and the interpreter must produce the result.
    let src = "$m = { a: 1, b: 2 }\n$i = 0\n$s = 0\nwhile $i < 10 {\n    $s = $s + $m[\"a\"]\n    $i = $i + 1\n}\n$s";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    let mut vm = VM::new();
    assert_eq!(vm.run(&bc).as_i48(), 10);
}

#[cfg(feature = "jit")]
#[test]
fn jit_compiles_map_field_loops() {
    let src = "$m = { a: 1, b: 2, c: 3 }\n$i = 0\n$s = 0\nwhile $i < 10 {\n    $s = $s + $m.a\n    $s = $s + $m.b\n    $s = $s + $m.c\n    $i = $i + 1\n}\n$s";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    let unit = prepare_jit(&bc).expect("map loop compiles");
    assert_eq!(unit.entries.len(), 1);
    assert_eq!(unit.entries[0].access_len, 3, "three field reads");
    let mut vm = VM::new();
    assert_eq!(vm.run(&bc).as_i48(), 60);

    // A non-integer field makes the cache fill fail; the interpreter must
    // still produce the right value.
    let src = "$m = { a: 1.5 }\n$i = 0\n$s = 0\nwhile $i < 10 {\n    $s = $s + $m.a\n    $i = $i + 1\n}\n$s";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    let mut vm = VM::new();
    assert_eq!(vm.run(&bc).as_f64(), 15.0);
}

#[cfg(feature = "jit")]
#[test]
fn jit_rejects_non_integer_loops() {
    // A float multiplication inside the body: outside the integer subset.
    let src = "$x = 1\n$i = 0\nwhile $i < 100 {\n    $x = $x * 1.5\n    $i = $i + 1\n}\n$x";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    assert!(prepare_jit(&bc).is_none(), "float loop must not compile");
    let mut vm = VM::new();
    assert!(vm.run(&bc).is_float());

    // A `for` loop is not a `while` header.
    let src = "$s = 0\nfor $i in 0..100 {\n    $s = $s + $i\n}\n$s";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    assert!(prepare_jit(&bc).is_none(), "range loop must not compile");
    let mut vm = VM::new();
    assert_eq!(vm.run(&bc).as_i48(), 4950);
}

#[cfg(feature = "jit")]
#[test]
fn jit_bails_out_without_committing_state() {
    // The counter starts as a float: the entry guard must bail and leave
    // the register file untouched for the interpreter.
    let src = "$i = 1.5\n$s = 0\nwhile $i < 5 {\n    $s = $s + 1\n    $i = $i + 1\n}\n$s";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    assert!(
        prepare_jit(&bc).is_some(),
        "loop is compilable; guard decides"
    );
    let mut vm = VM::new();
    assert_eq!(vm.run(&bc).as_i48(), 4);
}

#[test]
fn dropped_vm_buffers_are_reused_by_the_next_vm() {
    // A CLI creates one VM per script; the pool must hand the register file
    // and frame stack to the next VM instead of re-allocating them.
    let mut first = VM::new();
    let tokens = lex("1", "<test>");
    let ast = parse(&tokens, "1");
    let bc = compile(&ast);
    assert_eq!(first.run(&bc).as_i48(), 1);
    let regs = first.regs.as_ptr();
    let frames = first.frames.as_ptr();
    drop(first);

    let second = VM::new();
    assert_eq!(second.regs.as_ptr(), regs, "register file must be recycled");
    assert_eq!(
        second.frames.as_ptr(),
        frames,
        "frame stack must be recycled"
    );
}

#[test]
fn growing_the_register_file_rebases_active_frame_pointers() {
    // Each frame caches a raw pointer into `regs`. Growing the file moves
    // the buffer, so every live frame must be rebased; otherwise a caller
    // resumed after a deep callee reads and writes the freed block.
    let src = "1";
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);

    let mut vm = VM::new();
    // The initial capacity is small (and pooled between VMs), so the two
    // 40-slot bindings below are guaranteed to reallocate the file.
    assert!(
        vm.regs.capacity() < 80,
        "initial capacity assumption: {}",
        vm.regs.capacity()
    );
    let first = vm.acquire_frame(&bc);
    let base = vm.bind_registers(first, 40, 0);
    vm.regs[base] = Value::from_i48(7);

    // 40 + 40 exceeds the initial capacity, so this reallocates `regs`.
    let second = vm.acquire_frame(&bc);
    let _ = vm.bind_registers(second, 40, 0);

    assert_eq!(
        vm.frames[first].locals.ptr as usize,
        vm.regs.as_ptr() as usize,
        "a live frame must point at the current register file"
    );
    assert_eq!(vm.frames[first].locals[0].as_i48(), 7);
}

fn plugin_seven(_args: &[Value]) -> Value {
    Value::from_i48(7)
}

fn plugin_seven_hot(_args: &[Value]) -> Value {
    Value::from_i48(7)
}

static TEST_PLUGIN_DOMAINS: [DomainAlias; 1] = [DomainAlias {
    path: "std.util",
    domain: "util",
}];

static TEST_PLUGINS: [PluginDescriptor; 1] = [PluginDescriptor {
    name: "std",
    summary: "test plugin",
    domains: &TEST_PLUGIN_DOMAINS,
}];

static TEST_NATIVES: [crate::runtime::value::NativeEntry; 1] =
    [crate::runtime::value::NativeEntry {
        domain: Some("util"),
        impl_type: None,
        override_method: None,
        method: "seven",
        kind: crate::runtime::value::NativeKind::Pure,
        ptr: crate::runtime::value::RawPtr(plugin_seven as *const ()),
        args: None,
        ret: None,
        named: &[],
        summary: None,
        arg_desc: &[],
        auto_import: false,
        caps: crate::runtime::primitives::CapSet::EMPTY,
        hot: false,
    }];

static HOT_NATIVES: [crate::runtime::value::NativeEntry; 1] =
    [crate::runtime::value::NativeEntry {
        domain: Some("util"),
        impl_type: None,
        override_method: None,
        method: "seven_hot",
        kind: crate::runtime::value::NativeKind::Pure,
        ptr: crate::runtime::value::RawPtr(plugin_seven_hot as *const ()),
        args: None,
        ret: None,
        named: &[],
        summary: None,
        arg_desc: &[],
        auto_import: true,
        caps: crate::runtime::primitives::CapSet::EMPTY,
        hot: true,
    }];

/// Compiles `src` the way `xcl_vm::evaluate` does, against `vm`'s slot table.
fn compile_hot_for(src: &str, vm: &VM) -> crate::Bytecode {
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    crate::bytecode::compile_hot(&ast, vm.hot_table())
}

#[test]
fn vm_reports_which_plugin_owns_a_domain() {
    let mut vm = VM::new();
    vm.register_plugins(&TEST_PLUGINS);
    assert_eq!(vm.plugin_of("util"), Some("std"));
    assert_eq!(vm.plugin_of("std.util"), Some("std"));
    assert_eq!(vm.plugin_of("math"), None);
    assert!(vm.plugins().iter().any(|p| p.name == "std"));
}

#[test]
fn a_plugin_path_reaches_the_registered_domain() {
    let mut vm = VM::new();
    vm.register_plugins(&TEST_PLUGINS);
    vm.register_natives(&TEST_NATIVES);
    assert!(
        vm.resolve_domain_method("util", "seven").is_some(),
        "flat key must keep working"
    );
    assert!(
        vm.resolve_domain_method("std.util", "seven").is_some(),
        "qualified path must alias to the flat key"
    );
    assert!(vm.resolve_domain_method("std.math", "seven").is_none());
}

#[test]
fn hot_natives_get_a_slot_under_every_spelling() {
    let mut vm = VM::new();
    vm.register_plugins(&TEST_PLUGINS);
    vm.register_natives(&HOT_NATIVES);

    let slot = vm.hot_table().slot("seven_hot").expect("bare name");
    assert_eq!(vm.hot_table().slot("util.seven_hot"), Some(slot));
    assert_eq!(vm.hot_table().slot("std.util.seven_hot"), Some(slot));
    let native = vm.hot_native(slot as usize).expect("slot is populated");
    assert_eq!(native.domain, Some("util"));
    assert_eq!(native.method, "seven_hot");
}

#[test]
fn a_non_hot_native_keeps_its_name_call() {
    let mut vm = VM::new();
    vm.register_natives(&TEST_NATIVES);
    assert!(vm.hot_table().is_empty(), "nothing was declared hot");
    let bc = compile_hot_for("util.seven", &vm);
    assert!(
        bc.instructions
            .iter()
            .all(|i| i.op() != crate::bytecode::OP_CALL_HOT),
        "an unregistered slot must not be emitted"
    );
}

#[test]
fn a_hot_call_compiles_to_one_instruction_and_runs() {
    let mut vm = VM::new();
    vm.register_plugins(&TEST_PLUGINS);
    vm.register_natives(&HOT_NATIVES);

    let bc = compile_hot_for("seven_hot", &vm);
    assert!(
        bc.instructions
            .iter()
            .any(|i| i.op() == crate::bytecode::OP_CALL_HOT),
        "the call must use the hot slot"
    );
    assert_eq!(vm.run(&bc).as_i48(), 7);

    // The same program compiled without a slot table still reaches the native.
    let plain = {
        let tokens = lex("seven_hot", "<test>");
        let ast = parse(&tokens, "seven_hot");
        compile(&ast)
    };
    assert_eq!(vm.run(&plain).as_i48(), 7);
}

#[test]
fn an_ambiguous_hot_name_is_not_bound() {
    // Two auto-imported natives with the same bare name: the bare spelling
    // resolves to exactly one of them, so only that entry may claim it, and a
    // script that calls it keeps calling the same function it would by name.
    static DUPLICATES: [crate::runtime::value::NativeEntry; 2] = [
        crate::runtime::value::NativeEntry {
            domain: Some("util"),
            impl_type: None,
            override_method: None,
            method: "dup",
            kind: crate::runtime::value::NativeKind::Pure,
            ptr: crate::runtime::value::RawPtr(plugin_seven as *const ()),
            args: None,
            ret: None,
            named: &[],
            summary: None,
            arg_desc: &[],
            auto_import: true,
            caps: crate::runtime::primitives::CapSet::EMPTY,
            hot: true,
        },
        crate::runtime::value::NativeEntry {
            domain: Some("other"),
            impl_type: None,
            override_method: None,
            method: "dup",
            kind: crate::runtime::value::NativeKind::Pure,
            ptr: crate::runtime::value::RawPtr(plugin_seven_hot as *const ()),
            args: None,
            ret: None,
            named: &[],
            summary: None,
            arg_desc: &[],
            auto_import: true,
            caps: crate::runtime::primitives::CapSet::EMPTY,
            hot: true,
        },
    ];

    let mut vm = VM::new();
    vm.register_natives(&DUPLICATES);
    let bc = compile_hot_for("dup", &vm);
    assert_eq!(vm.run(&bc).as_i48(), 7);
}

// ── Loop rotation and hot-call hoisting ──

/// Compiles and runs `src`, returning both the result and the bytecode.
fn eval_with_code(src: &str) -> (Value, crate::Bytecode) {
    let tokens = lex(src, "<test>");
    let ast = parse(&tokens, src);
    let bc = compile(&ast);
    let mut vm = VM::new();
    let result = vm.run(&bc);
    core::mem::forget(vm);
    (result, bc)
}

fn rotates(bc: &crate::Bytecode) -> bool {
    bc.instructions.iter().any(|i| {
        i.op() == crate::bytecode::OP_INC_LT_INT || i.op() == crate::bytecode::OP_INC_LT_INT_WIDE
    })
}

#[test]
fn a_counted_loop_is_rotated_and_still_counts() {
    // `$s` starts from the argument count so the loop cannot be folded away;
    // rotation and folding are independent passes.
    let src = "$s = $#
$i = 0
while $i < 10 {
  $s = $s + 2
  $i = $i + 1
}
$s";
    let (result, bc) = eval_with_code(src);
    assert!(rotates(&bc), "the canonical counted loop must rotate");
    assert_eq!(result.as_i48(), 20);
}

#[test]
fn a_counted_sum_loop_is_folded_away() {
    let src = "$i = 0
$s = 0
while $i < 10000 {
  $s = $s + $i
  $i = $i + 1
}
$s * 10 + $i";
    let (result, bc) = eval_with_code(src);
    assert!(
        bc.instructions
            .iter()
            .all(|i| i.op() != OP_WHILE_LT_INT && i.op() != OP_WHILE_LT_INT_WIDE),
        "a proven counted loop must not survive"
    );
    assert_eq!(result.as_i48(), 49_995_000 * 10 + 10_000);
}

#[test]
fn a_range_sum_is_folded_away() {
    let src = "$s = 0
for $i in 0..10000 {
  $s = $s + $i
}
$s + $i";
    let (result, bc) = eval_with_code(src);
    assert!(bc.instructions.iter().all(|i| i.op() != OP_FOR_NUMBER_LOOP));
    assert_eq!(result.as_i48(), 49_995_000 + 10_000);
}

#[test]
fn several_accumulators_and_constant_terms_fold() {
    let src = "$i = 0
$a = 0
$b = 0
while $i < 5 {
  $a = $a + $i
  $b = $b + 3
  $a = $a + 1
  $i = $i + 1
}
$a * 100 + $b";
    let (result, _bc) = eval_with_code(src);
    // a = sum(0..4) + 5 = 10 + 5 = 15, b = 3 * 5 = 15
    assert_eq!(result.as_i48(), 1515);
}

#[test]
fn a_multiplicative_body_is_not_folded() {
    let src = "$i = 0
$s = 1
while $i < 5 {
  $s = $s * 2
  $i = $i + 1
}
$s";
    let (result, _bc) = eval_with_code(src);
    assert_eq!(result.as_i48(), 32);
}

#[test]
fn a_fold_that_would_overflow_the_48_bit_range_is_skipped() {
    // The accumulator starts near `i48::MAX`, so the total does not fit and the
    // loop must survive; the counter still reaches its bound.
    let src = "$s = 140737488355000
$i = 0
while $i < 100 {
  $s = $s + $i
  $i = $i + 1
}
$i";
    let (result, bc) = eval_with_code(src);
    assert!(
        bc.instructions
            .iter()
            .any(|i| i.op() == OP_WHILE_LT_INT),
        "the loop must stay when its total does not fit"
    );
    assert_eq!(result.as_i48(), 100);
}

#[test]
fn a_fold_keeps_the_counter_and_accumulator_values() {
    let src = "$i = 3
$s = 0
while $i < 8 {
  $s = $s + $i
  $i = $i + 1
}
$s * 100 + $i";
    let (result, _bc) = eval_with_code(src);
    assert_eq!(result.as_i48(), (3 + 4 + 5 + 6 + 7) * 100 + 8);
}

#[test]
fn a_loop_with_a_local_limit_is_not_rotated() {
    // Rotation needs the compile-time limit of `WHILE_LT_INT`; a limit
    // that lives in a local compiles to `WHILE_LT_LOCAL` and keeps its shape.
    let src = "$n = 4
$i = 0
$s = 0
while $i < $n {
  $s = $s + 2
  $i = $i + 1
}
$s";
    let (result, bc) = eval_with_code(src);
    assert!(!rotates(&bc));
    assert_eq!(result.as_i48(), 8);
}

#[test]
fn nested_counted_loops_both_rotate() {
    let src = "$i = 0
$s = 0
while $i < 4 {
  $j = 0
  while $j < 3 {
    $s = $s + 1
    $j = $j + 1
  }
  $i = $i + 1
}
$s";
    let (result, bc) = eval_with_code(src);
    assert!(rotates(&bc), "the inner counted loop must rotate");
    assert_eq!(result.as_i48(), 12);
}

#[test]
fn a_counter_written_in_the_body_blocks_rotation() {
    let src = "$i = 0\n$s = 0\nwhile $i < 5 {\n  $s = $s + 1\n  if $s == 3 { $i = $i + 2 }\n  $i = $i + 1\n}\n$s";
    let (result, bc) = eval_with_code(src);
    assert!(!rotates(&bc), "a body that writes the counter must stay as written");
    assert_eq!(result.as_i48(), 3);
}

#[test]
fn a_descending_loop_keeps_its_shape() {
    let src = "$i = 4
$s = 0
while $i > 0 {
  $s = $s + $i
  $i = $i - 1
}
$s";
    let (result, bc) = eval_with_code(src);
    assert!(!rotates(&bc), "rotation covers ascending `$i < N` loops only");
    assert_eq!(result.as_i48(), 10);
}

#[test]
fn a_zero_trip_rotated_loop_runs_zero_times() {
    let src = "$i = 5\n$s = 0\nwhile $i < 5 {\n  $s = $s + 1\n  $i = $i + 1\n}\n$s";
    let (result, bc) = eval_with_code(src);
    assert!(rotates(&bc));
    assert_eq!(result.as_i48(), 0);
}

// ── break / continue ──

#[test]
fn break_leaves_a_fused_while() {
    let src = "$i = 0\n$s = 0\nwhile $i < 100 {\n  $s = $s + 1\n  if $s == 7 { break }\n  $i = $i + 1\n}\n$s";
    let (result, _bc) = eval_with_code(src);
    assert_eq!(result.as_i48(), 7);
}

#[test]
fn break_leaves_a_generic_while() {
    // A condition that is not a comparison keeps the generic loop shape.
    let src = "$s = 0\n$go = true\nwhile $go {\n  $s = $s + 1\n  if $s == 3 { break }\n}\n$s";
    let (result, _bc) = eval_with_code(src);
    assert_eq!(result.as_i48(), 3);
}

#[test]
fn continue_skips_the_rest_of_the_body() {
    let src = "$i = 0\n$s = 0\nwhile $i < 10 {\n  $i = $i + 1\n  if $i == 3 { continue }\n  $s = $s + 1\n}\n$i * 100 + $s";
    let (result, _bc) = eval_with_code(src);
    assert_eq!(result.as_i48(), 1009, "i == 10 and 9 counted iterations");
}

#[test]
fn break_leaves_a_numeric_for() {
    let src = "$s = 0\nfor $i in 0..10 {\n  if $i == 4 { break }\n  $s = $s + 1\n}\n$s";
    let (result, _bc) = eval_with_code(src);
    assert_eq!(result.as_i48(), 4);
}

#[test]
fn continue_steps_a_numeric_for() {
    let src = "$s = 0\nfor $i in 0..10 {\n  if $i % 2 == 0 { continue }\n  $s = $s + 1\n}\n$s";
    let (result, _bc) = eval_with_code(src);
    assert_eq!(result.as_i48(), 5, "1, 3, 5, 7, 9");
}

#[test]
fn break_and_continue_belong_to_the_innermost_loop() {
    let src = "$s = 0\nfor $i in 0..3 {\n  for $j in 0..10 {\n    if $j == 2 { break }\n    $s = $s + 1\n  }\n}\n$s";
    let (result, _bc) = eval_with_code(src);
    assert_eq!(result.as_i48(), 6, "two inner iterations per outer pass");
}

#[test]
fn a_break_inside_a_lambda_does_not_leave_the_outer_loop() {
    let src = "$s = 0\n$f = |$z| { break }\nwhile $s < 3 {\n  $s = $s + 1\n}\n$s";
    let (result, _bc) = eval_with_code(src);
    assert_eq!(result.as_i48(), 3);
}

#[test]
fn a_continue_does_not_block_rotation() {
    // `continue` jumps to the entry test, which rotation keeps, so the loop may
    // still rotate; the skipped iteration must not advance the counter.
    let src = "$i = 0\n$s = 0\n$k = 0\nwhile $i < 5 {\n  $k = $k + 1\n  if $k == 2 { continue }\n  $s = $s + 1\n  $i = $i + 1\n}\n$s";
    let (result, bc) = eval_with_code(src);
    assert!(rotates(&bc), "the latch is still the loop's last instruction");
    assert_eq!(result.as_i48(), 5);
}
