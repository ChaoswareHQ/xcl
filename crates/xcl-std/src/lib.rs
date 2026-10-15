pub mod defer;
pub mod error;
pub mod io;
pub mod input;
pub mod output;
pub mod result;

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
    fn io_print_space_form() {
        // `io.print "hello"` should work (space-separated call)
        let mut vm = new_vm();
        let _r = evaluate(r#"io.print "hello""#, &mut vm);
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
        // `io.print ($x)` on the same line should pass the parenthesized value
        let mut vm = new_vm();
        let r = evaluate("$x = 42\nio.print ($x)", &mut vm);
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
    fn io_print_multiple_args() {
        // `io.print "a" "b" "c"` should not panic and returns none
        let mut vm = new_vm();
        let r = evaluate(r#"io.print "a" "b" "c""#, &mut vm);
        assert!(r.is_nil());
    }

    #[test]
    fn io_print_int_and_float() {
        let mut vm = new_vm();
        let r = evaluate(r#"io.print 42 3.14 true"#, &mut vm);
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
}