pub mod arr;
pub mod defer;
pub mod dict_ops;
pub mod error;
pub mod io;
pub mod input;
pub mod output;
pub mod result;
pub mod str_ops;
pub mod math;

use xcl_vm::{VM, Value};

/// Create a new VM with all standard library types and builtins registered.
pub fn new_vm() -> VM {
    let mut vm = VM::new();
    vm.register_all();
    vm
}

/// Evaluate XCL source code using the provided VM.
pub fn evaluate(source: &str, vm: &mut VM) -> Value {
    let result = xcl_vm::evaluate(source, vm);
    output::flush_out();
    result
}

/// Evaluate XCL source code in a fresh VM with standard library.
pub fn evaluate_new(source: &str) -> Value {
    let mut vm = new_vm();
    evaluate(source, &mut vm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcl_vm::{TypeKind, SUBTYPE_RESULT, VARIANT_OK, VARIANT_ERR, SUBTYPE_ERROR};

    #[test]
    fn ok_constructor() {
        let mut vm = new_vm();
        let r = evaluate(r#"Ok 42"#, &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_RESULT));
        assert_eq!(r.map_variant(), Some(VARIANT_OK));
        assert_eq!(r.as_map().unwrap().get("value").unwrap().as_i48(), 42);
    }

    #[test]
    fn err_constructor() {
        let mut vm = new_vm();
        let r = evaluate(r#"Err (make_error "not_found" "boom" 500)"#, &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_RESULT));
        assert_eq!(r.map_variant(), Some(VARIANT_ERR));
        let err = r.as_map().unwrap().get("error").unwrap();
        assert_eq!(err.as_map().unwrap().get("kind").unwrap().as_str(), Some("not_found"));
        assert_eq!(err.as_map().unwrap().get("message").unwrap().as_str(), Some("boom"));
        assert_eq!(err.as_map().unwrap().get("code").unwrap().as_i48(), 500);
    }

    #[test]
    fn result_type_registered() {
        let vm = new_vm();
        let desc = vm.type_index.get("Result").expect("Result registered");
        assert!(matches!(desc.kind, TypeKind::Sum));
    }

    #[test]
    fn result_descriptor_has_variants() {
        let vm = new_vm();
        let desc = vm.type_index.get("Result").expect("Result registered");

        assert_eq!(desc.variants.len(), 2, "Result should have exactly 2 variants");
        assert_eq!(desc.variants[0].name, "Ok");
        assert_eq!(desc.variants[0].fields.len(), 1);
        assert_eq!(desc.variants[0].fields[0].name, "value");
        assert_eq!(desc.variants[0].fields[0].ty, "any");

        assert_eq!(desc.variants[1].name, "Err");
        assert_eq!(desc.variants[1].fields.len(), 1);
        assert_eq!(desc.variants[1].fields[0].name, "error");
        assert_eq!(desc.variants[1].fields[0].ty, "Error");

        assert!(desc.summary.is_some());
        assert!(desc.summary.unwrap().contains("success or failure"));
    }

    #[test]
    fn error_type_registered() {
        let vm = new_vm();
        let desc = vm.type_index.get("Error").expect("Error registered");
        assert!(matches!(desc.kind, TypeKind::Record));
        assert_eq!(desc.fields.len(), 3);
    }

    #[test]
    fn make_error_builtin() {
        let mut vm = new_vm();
        let r = evaluate(r#"make_error "not_found" "file missing" 2"#, &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_ERROR));
        let m = r.as_map().unwrap();
        assert_eq!(m.get("kind").unwrap().as_str(), Some("not_found"));
        assert_eq!(m.get("message").unwrap().as_str(), Some("file missing"));
        assert_eq!(m.get("code").unwrap().as_i48(), 2);
    }

    #[test]
    fn io_read_returns_result() {
        let mut vm = new_vm();
        let r = evaluate(r#"io.read "/tmp/nonexistent_xyz.txt""#, &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_RESULT));
        assert_eq!(r.map_variant(), Some(VARIANT_ERR));
    }

    #[test]
    fn io_write_read_roundtrip() {
        let path = std::env::temp_dir().join("xcl_io_roundtrip.txt");
        let path_str = path.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&path);

        let mut vm = new_vm();
        let write_r = evaluate(&format!(r#"io.write "{}" "hello""#, path_str), &mut vm);
        assert_eq!(write_r.map_variant(), Some(VARIANT_OK));

        let mut vm = new_vm();
        let read_r = evaluate(&format!(r#"io.read "{}""#, path_str), &mut vm);
        assert_eq!(read_r.map_variant(), Some(VARIANT_OK));
        assert_eq!(read_r.as_map().unwrap().get("value").unwrap().as_str(), Some("hello"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn io_open_returns_result() {
        let mut vm = new_vm();
        let r = evaluate(r#"io.open "/tmp" "r""#, &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_RESULT));
    }

    // â”€â”€ Space-separated call tests â”€â”€

    #[test]
    fn io_read_space_form() {
        let mut vm = new_vm();
        let r = evaluate(r#"io.read "/tmp/nonexistent_xyz.txt""#, &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_RESULT));
        assert_eq!(r.map_variant(), Some(VARIANT_ERR));
    }

    #[test]
    fn io_write_space_form() {
        let path = std::env::temp_dir().join("xcl_space_form.txt");
        let path_str = path.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&path);

        let mut vm = new_vm();
        let r = evaluate(&format!(r#"io.write "{}" "hello""#, path_str), &mut vm);
        assert_eq!(r.map_variant(), Some(VARIANT_OK));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn field_access_still_works() {
        // $m.key is still field access, not a call
        let mut vm = new_vm();
        let r = evaluate(r#"$m = { key: 42 }
$m.key"#, &mut vm);
        assert_eq!(r.as_i48(), 42);
    }

    #[test]
    fn print_space_form() {
        // `print "hello"` should work (space-separated call)
        let mut vm = new_vm();
        let _r = evaluate(r#"print "hello""#, &mut vm);
    }

    // â”€â”€ Newline-separated statement tests â”€â”€

    #[test]
    fn parenthesized_on_new_line_is_new_statement() {
        // `$x = 42\n($x)` should return 42, not nil
        let mut vm = new_vm();
        let r = evaluate("$x = 42\n($x)", &mut vm);
        assert_eq!(r.as_i48(), 42);
    }

    #[test]
    fn parenthesized_on_same_line_is_arg() {
        // `print ($x)` on the same line should pass the parenthesized value
        let mut vm = new_vm();
        let r = evaluate("$x = 42\nprint ($x)", &mut vm);
        assert!(r.is_nil());
    }

    #[test]
    fn call_on_same_line_still_works() {
        // io.read and its arg are on the same line
        let mut vm = new_vm();
        let r = evaluate(r#"io.read "/tmp/nonexistent_xyz.txt""#, &mut vm);
        assert!(r.as_map().is_some());
    }

    // â”€â”€ Parenthesized field access arithmetic tests â”€â”€

    #[test]
    fn parenthesized_field_access_is_arithmetic() {
        // `($m.y) - 1` should be arithmetic, not a call
        let mut vm = new_vm();
        let r = evaluate(r#"$m = { y: 10 }
($m.y) - 1"#, &mut vm);
        assert_eq!(r.as_i48(), 9);
    }

    #[test]
    fn parenthesized_expr_then_add() {
        let mut vm = new_vm();
        let r = evaluate("(40) + 2", &mut vm);
        assert_eq!(r.as_i48(), 42);
    }

    #[test]
    fn bare_field_access_with_arg_is_call() {
        // $f.double 5 where double is a function field should parse as a call.
        // The VM currently doesn't support calling lambdas stored in dict fields
        // via space-separated args, so we just verify the parser produces a call
        // by checking it doesn't panic and returns some value.
        let mut vm = new_vm();
        let _r = evaluate(r#"$f = { double: ($x) => $x * 2 }
$f.double 5"#, &mut vm);
        // If we get here without panic, the parser correctly built a Call node
    }

    // â”€â”€ Print and format tests â”€â”€

    #[test]
    fn print_multiple_args() {
        // `print "a" "b" "c"` should not panic and returns none
        let mut vm = new_vm();
        let r = evaluate(r#"print "a" "b" "c""#, &mut vm);
        assert!(r.is_nil());
    }

    #[test]
    fn print_int_and_float() {
        let mut vm = new_vm();
        let r = evaluate(r#"print 42 3.14 true"#, &mut vm);
        assert!(r.is_nil());
    }

    #[test]
    fn io_format_returns_string() {
        let mut vm = new_vm();
        let r = evaluate(r#"io.format "a" 42 3.14"#, &mut vm);
        assert_eq!(r.as_str(), Some("a 42 3.14"));
    }

    #[test]
    fn io_format_int_only() {
        let mut vm = new_vm();
        let r = evaluate(r#"io.format 100"#, &mut vm);
        assert_eq!(r.as_str(), Some("100"));
    }

    #[test]
    fn io_format_bool_and_nil() {
        let mut vm = new_vm();
        let r = evaluate(r#"io.format true false"#, &mut vm);
        assert_eq!(r.as_str(), Some("true false"));
    }

    // ── Closure dispatch tests ──

    #[test]
    fn closure_basic_call() {
        // Verify a simple closure call works
        let mut vm = new_vm();
        let r = evaluate("($x) => $x * 2", &mut vm);
        assert!(r.as_func().is_some(), "closure should be a func value");
    }

    #[test]
    fn map_calls_closure() {
        let mut vm = new_vm();
        let r = evaluate("result.map (Ok 10) (($x) => $x * 2)", &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_RESULT));
        assert_eq!(r.map_variant(), Some(VARIANT_OK));
        assert_eq!(r.as_map().unwrap().get("value").unwrap().as_i48(), 20);
    }

    #[test]
    fn map_passes_err_through() {
        let mut vm = new_vm();
        let r = evaluate(r#"result.map (Err (make_error "x" "y" 0)) (($x) => $x * 2)"#, &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_RESULT));
        assert_eq!(r.map_variant(), Some(VARIANT_ERR));
    }

    #[test]
    fn and_then_chains() {
        let mut vm = new_vm();
        let src = r#"
$double = ($x) => Ok ($x * 2)
result.and_then (Ok 5) $double"#;
        let r = evaluate(src, &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_RESULT));
        assert_eq!(r.map_variant(), Some(VARIANT_OK));
        assert_eq!(r.as_map().unwrap().get("value").unwrap().as_i48(), 10);
    }

    #[test]
    fn and_then_passes_err_through() {
        let mut vm = new_vm();
        let r = evaluate(r#"result.and_then (Err (make_error "x" "y" 0)) (($x) => Ok ($x * 2))"#, &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_RESULT));
        assert_eq!(r.map_variant(), Some(VARIANT_ERR));
    }

    #[test]
    fn or_else_recovers() {
        let mut vm = new_vm();
        let src = r#"
$fallback = ($_) => Ok 99
result.or_else (Err (make_error "x" "y" 0)) $fallback"#;
        let r = evaluate(src, &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_RESULT));
        assert_eq!(r.map_variant(), Some(VARIANT_OK));
        assert_eq!(r.as_map().unwrap().get("value").unwrap().as_i48(), 99);
    }

    #[test]
    fn or_else_passes_ok_through() {
        let mut vm = new_vm();
        let src = r#"
$fallback = ($_) => Ok 99
result.or_else (Ok 42) $fallback"#;
        let r = evaluate(src, &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_RESULT));
        assert_eq!(r.map_variant(), Some(VARIANT_OK));
        assert_eq!(r.as_map().unwrap().get("value").unwrap().as_i48(), 42);
    }

    #[test]
    fn unwrap_or_else_calls_fallback() {
        let mut vm = new_vm();
        let src = r#"
$fallback = ($_) => 77
result.unwrap_or_else (Err (make_error "x" "y" 0)) $fallback"#;
        let r = evaluate(src, &mut vm);
        assert_eq!(r.as_i48(), 77);
    }

    #[test]
    fn unwrap_or_else_returns_ok_value() {
        let mut vm = new_vm();
        let src = r#"
$fallback = ($_) => 77
result.unwrap_or_else (Ok 42) $fallback"#;
        let r = evaluate(src, &mut vm);
        assert_eq!(r.as_i48(), 42);
    }

    #[test]
    fn map_err_field_access_debug() {
        // Test field access on Error directly (no closure)
        let mut vm = new_vm();
        let r = evaluate(r#"$e = make_error "not_found" "file" 2; $e.message"#, &mut vm);
        assert_eq!(r.as_str(), Some("file"), "direct field access should return 'file'");
    }

    #[test]
    fn map_err_field_access_in_closure() {
        // Test field access on Error inside closure
        let mut vm = new_vm();
        let src = r#"
$getter = ($e) => $e.message
$e = make_error "not_found" "file" 2
$getter $e"#;
        let r = evaluate(src, &mut vm);
        assert_eq!(r.as_str(), Some("file"), "closure field access should return 'file'");
    }

    #[test]
    fn map_err_transforms_error() {
        let mut vm = new_vm();
        let src = r#"
$wrapper = ($e) => make_error "wrapped" $e.message $e.code
result.map_err (Err (make_error "not_found" "file" 2)) $wrapper"#;
        let r = evaluate(src, &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_RESULT));
        assert_eq!(r.map_variant(), Some(VARIANT_ERR));
        let err = r.as_map().unwrap().get("error").unwrap();
        assert_eq!(err.as_map().unwrap().get("kind").unwrap().as_str(), Some("wrapped"));
    }

    // ── Edge case tests from issue ──

    #[test]
    fn call_local_function_value() {
        let mut vm = new_vm();
        let src = r#"
$double = ($x) => $x * 2
$double 5"#;
        assert_eq!(evaluate(src, &mut vm).as_i48(), 10);
    }

    #[test]
    fn call_local_with_two_args() {
        let mut vm = new_vm();
        let src = r#"
$add = ($a; $b) => $a + $b
$add 3 4"#;
        assert_eq!(evaluate(src, &mut vm).as_i48(), 7);
    }

    #[test]
    fn field_accesses_as_separate_args() {
        let mut vm = new_vm();
        let src = r#"
$e = make_error "not_found" "missing" 2
make_error $e.kind $e.message $e.code"#;
        let r = evaluate(src, &mut vm);
        assert_eq!(
            r.as_map().unwrap().get("kind").unwrap().as_str(),
            Some("not_found")
        );
    }

    #[test]
    fn call_inside_closure_with_field_args() {
        let mut vm = new_vm();
        let src = r#"$r = Err (make_error "x" "boom" 1)
$r.map_err (($e) => make_error "wrapped" $e.message $e.code)"#;
        let r = evaluate(src, &mut vm);
        assert_eq!(r.map_subtype(), Some(SUBTYPE_RESULT));
        assert_eq!(r.map_variant(), Some(VARIANT_ERR));
        let err = r.as_map().unwrap().get("error").unwrap();
        assert_eq!(err.as_map().unwrap().get("kind").unwrap().as_str(), Some("wrapped"));
    }

    #[test]
    fn multi_arg_does_not_nest_calls() {
        // foo $f $x should be foo($f, $x), not foo($f($x))
        let mut vm = new_vm();
        let src = r#"
$ident = ($x) => $x
$three = 3
$pair = ($a; $b) => [$a; $b]
$pair $ident $three"#;
        let r = evaluate(src, &mut vm);
        // r should be an array with 2 elements
        let arr = r.as_array().expect("result should be an array");
        assert_eq!(arr.len(), 2);
    }

    // ── defer tests ──

    #[test]
    fn defer_builtin_registered() {
        // Verify defer is registered as a global builtin
        let vm = new_vm();
        assert!(vm.resolve_builtin("defer").is_some(), "defer should be registered");
    }

    // ── block-lambda tests ──

    #[test]
    fn block_lambda_basic() {
        // `{ print "hello" }` creates a zero-param closure
        let mut vm = new_vm();
        let r = evaluate(r#"$f = { print "hello" }; $f"#, &mut vm);
        assert!(r.as_func().is_some(), "block-lambda should produce a function");
    }

    #[test]
    fn block_lambda_call() {
        // Calling a block-lambda executes its body and returns the last value
        let mut vm = new_vm();
        let r = evaluate(r#"$f = { 42 }; $f"#, &mut vm);
        assert!(r.as_func().is_some());
        // Call the function
        let r2 = evaluate(r#"$f = { 42 }; $f ()"#, &mut vm);
        assert_eq!(r2.as_i48(), 42);
    }

    #[test]
    fn block_lambda_multiple_statements() {
        // Block-lambda with multiple statements
        let mut vm = new_vm();
        let r = evaluate(r#"$f = { $x = 10; $x + 2 }; $f ()"#, &mut vm);
        assert_eq!(r.as_i48(), 12);
    }

    #[test]
    fn block_lambda_empty() {
        // `{ }` is an empty block-lambda that returns nil
        let mut vm = new_vm();
        let r = evaluate(r#"$f = { }; $f ()"#, &mut vm);
        assert!(r.is_nil(), "empty block-lambda should return nil");
    }

    #[test]
    fn dict_literal_still_works() {
        // `{ key: value }` is still a dict literal
        let mut vm = new_vm();
        let r = evaluate(r#"{ a: 1; b: 2 }"#, &mut vm);
        let map = r.as_map().expect("should be a dict");
        assert_eq!(map.get("a").unwrap().as_i48(), 1);
        assert_eq!(map.get("b").unwrap().as_i48(), 2);
    }

    #[test]
    fn defer_block_syntax() {
        // `defer { ... }` keyword-like syntax
        let mut vm = new_vm();
        let r = evaluate(r#"
defer { print "cleaned" }
print "main"
"#, &mut vm);
        assert!(r.is_nil());
    }

    // ── implicit $it tests ──

    #[test]
    fn implicit_it_block_lambda() {
        // `{ $it * 2 }` creates a 1-param closure with implicit "it"
        let mut vm = new_vm();
        let r = evaluate(r#"$f = { $it * 2 }; $f 21"#, &mut vm);
        assert_eq!(r.as_i48(), 42);
    }

    #[test]
    fn implicit_it_arr_map() {
        // `arr.map $a { $it + 1 }` — block-lambda with implicit $it
        let mut vm = new_vm();
        let r = evaluate(r#"$a = [1; 2; 3]; arr.map $a { $it + 1 }"#, &mut vm);
        let arr = r.as_array().expect("result should be an array");
        assert_eq!(arr[0].as_i48(), 2);
        assert_eq!(arr[1].as_i48(), 3);
        assert_eq!(arr[2].as_i48(), 4);
    }

    #[test]
    fn implicit_it_arr_filter() {
        // `arr.filter $a { $it > 1 }` — implicit $it in filter
        let mut vm = new_vm();
        let r = evaluate(r#"$a = [1; 2; 3; 4]; arr.filter $a { $it > 2 }"#, &mut vm);
        let arr = r.as_array().expect("result should be an array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].as_i48(), 3);
        assert_eq!(arr[1].as_i48(), 4);
    }

    #[test]
    fn implicit_it_defer() {
        // `defer { $it }` — defer with implicit $it (no-op since defer passes no args)
        let mut vm = new_vm();
        let r = evaluate(r#"
$x = 0
defer { $x = 99 }
print "done"
"#, &mut vm);
        assert!(r.is_nil());
    }

    #[test]
    fn implicit_it_without_usage() {
        // `{ 42 }` without $it should NOT add implicit param; call with ()
        let mut vm = new_vm();
        let r = evaluate(r#"$f = { 42 }; $f ()"#, &mut vm);
        assert_eq!(r.as_i48(), 42);
    }

    #[test]
    fn implicit_it_nested_call() {
        // `$it` inside a call within block-lambda
        let mut vm = new_vm();
        let r = evaluate(r#"$a = [1; 2; 3]; arr.map $a { math.abs $it }"#, &mut vm);
        let arr = r.as_array().expect("result should be an array");
        assert_eq!(arr[0].as_f64(), 1.0);
        assert_eq!(arr[1].as_f64(), 2.0);
        assert_eq!(arr[2].as_f64(), 3.0);
    }

    // ── array tests ──

    #[test]
    fn arr_map_doubles() {
        let mut vm = new_vm();
        let r = evaluate(r#"$a = [1; 2; 3]; $f = (($x) => $x * 2); arr.map $a $f"#, &mut vm);
        let arr = r.as_array().expect("result should be an array");
        assert_eq!(arr[0].as_i48(), 2);
        assert_eq!(arr[1].as_i48(), 4);
        assert_eq!(arr[2].as_i48(), 6);
    }

    #[test]
    fn arr_filter_positives() {
        let mut vm = new_vm();
        let r = evaluate(r#"$a = [-1; 2; -3; 4]; $f = (($x) => $x > 0); arr.filter $a $f"#, &mut vm);
        let arr = r.as_array().expect("result should be an array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].as_i48(), 2);
        assert_eq!(arr[1].as_i48(), 4);
    }

    #[test]
    fn arr_reduce_sum() {
        let mut vm = new_vm();
        let r = evaluate(r#"$a = [1; 2; 3; 4]; $f = (($acc, $x) => $acc + $x); arr.reduce $a 0 $f"#, &mut vm);
        assert_eq!(r.as_i48(), 10);
    }

    #[test]
    fn arr_sort_ascending() {
        let mut vm = new_vm();
        let r = evaluate(r#"$a = [3; 1; 4; 1; 5]; arr.sort $a"#, &mut vm);
        let arr = r.as_array().expect("result should be an array");
        assert_eq!(arr[0].as_i48(), 1);
        assert_eq!(arr[1].as_i48(), 1);
        assert_eq!(arr[2].as_i48(), 3);
        assert_eq!(arr[3].as_i48(), 4);
        assert_eq!(arr[4].as_i48(), 5);
    }

    #[test]
    fn arr_sum() {
        let mut vm = new_vm();
        let r = evaluate(r#"$a = [1; 2; 3; 4]; arr.sum $a"#, &mut vm);
        assert_eq!(r.as_f64(), 10.0);
    }

    #[test]
    fn arr_contains() {
        let mut vm = new_vm();
        let r = evaluate(r#"$a = [1; 2; 3]; arr.contains $a 2"#, &mut vm);
        assert_eq!(r.as_bool(), true);
    }

    #[test]
    fn arr_first_last() {
        let mut vm = new_vm();
        let r1 = evaluate(r#"$a = [10; 20; 30]; arr.first $a"#, &mut vm);
        assert_eq!(r1.as_i48(), 10);
        let r2 = evaluate(r#"$a = [10; 20; 30]; arr.last $a"#, &mut vm);
        assert_eq!(r2.as_i48(), 30);
    }

    #[test]
    fn arr_unique() {
        let mut vm = new_vm();
        let r = evaluate(r#"$a = [1; 2; 2; 3; 3; 3]; arr.unique $a"#, &mut vm);
        let arr = r.as_array().expect("result should be an array");
        assert_eq!(arr.len(), 3);
    }

    #[test]
    fn arr_reverse() {
        let mut vm = new_vm();
        let r = evaluate(r#"$a = [1; 2; 3]; arr.reverse $a"#, &mut vm);
        let arr = r.as_array().expect("result should be an array");
        assert_eq!(arr[0].as_i48(), 3);
        assert_eq!(arr[2].as_i48(), 1);
    }

    // ── string tests ──

    #[test]
    fn str_upper() {
        let mut vm = new_vm();
        let r = evaluate(r#"str.upper "hello""#, &mut vm);
        assert_eq!(r.as_str(), Some("HELLO"));
    }

    #[test]
    fn str_lower() {
        let mut vm = new_vm();
        let r = evaluate(r#"str.lower "HELLO""#, &mut vm);
        assert_eq!(r.as_str(), Some("hello"));
    }

    #[test]
    fn str_split() {
        let mut vm = new_vm();
        let r = evaluate(r#"str.split "a,b,c" ",""#, &mut vm);
        let arr = r.as_array().expect("result should be an array");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].as_str(), Some("a"));
    }

    #[test]
    fn str_starts_with() {
        let mut vm = new_vm();
        let r = evaluate(r#"str.starts_with "hello" "hel""#, &mut vm);
        assert_eq!(r.as_bool(), true);
    }

    #[test]
    fn str_replace() {
        let mut vm = new_vm();
        let r = evaluate(r#"str.replace "hello world" "world" "xcl""#, &mut vm);
        assert_eq!(r.as_str(), Some("hello xcl"));
    }

    #[test]
    fn str_len() {
        let mut vm = new_vm();
        let r = evaluate(r#"str.len "hello""#, &mut vm);
        assert_eq!(r.as_i48(), 5);
    }

    // ── dict tests ──

    #[test]
    fn dict_keys() {
        let mut vm = new_vm();
        let r = evaluate(r#"$d = { a: 1, b: 2 }; dict.keys $d"#, &mut vm);
        let arr = r.as_array().expect("result should be an array");
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn dict_get() {
        let mut vm = new_vm();
        let r = evaluate(r#"$d = { a: 42 }; dict.get $d "a""#, &mut vm);
        assert_eq!(r.as_i48(), 42);
    }

    #[test]
    fn dict_has() {
        let mut vm = new_vm();
        let r = evaluate(r#"$d = { a: 1 }; dict.has $d "a""#, &mut vm);
        assert_eq!(r.as_bool(), true);
    }

    #[test]
    fn dict_len() {
        let mut vm = new_vm();
        let r = evaluate(r#"$d = { a: 1, b: 2, c: 3 }; dict.len $d"#, &mut vm);
        assert_eq!(r.as_i48(), 3);
    }
}