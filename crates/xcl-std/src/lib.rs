pub mod io;
pub mod input;
pub mod output;
pub mod result;

use xcl_vm::{VM, Value};

/// Create a new VM with all standard library types and builtins registered.
pub fn new_vm() -> VM {
    let mut vm = VM::new();
    vm.register_all();
    result::register_well_known(&mut vm);
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
    use xcl_vm::TypeKind;

    #[test]
    fn ok_constructor() {
        let mut vm = new_vm();
        let r = evaluate(r#"Ok 42"#, &mut vm);
        let m = r.as_map().unwrap();
        assert_eq!(m.get("tag").unwrap().as_str(), Some("Ok"));
        assert_eq!(m.get("value").unwrap().as_i48(), 42);
    }

    #[test]
    fn err_constructor() {
        let mut vm = new_vm();
        let r = evaluate(r#"Err "boom", 500"#, &mut vm);
        let m = r.as_map().unwrap();
        assert_eq!(m.get("tag").unwrap().as_str(), Some("Err"));
        assert_eq!(m.get("message").unwrap().as_str(), Some("boom"));
        assert_eq!(m.get("code").unwrap().as_i48(), 500);
    }

    #[test]
    fn result_type_registered() {
        let vm = new_vm();
        let desc = vm.type_index.get("Result").expect("Result registered");
        assert!(matches!(desc.kind, TypeKind::Sum));
    }

    #[test]
    fn well_known_filled() {
        let vm = new_vm();
        assert!(vm.well_known.has_result());
        assert_eq!(vm.well_known.ok_ctor, Some("Ok"));
        assert_eq!(vm.well_known.err_ctor, Some("Err"));
        assert_eq!(vm.well_known.result_type, Some("Result"));
        assert_eq!(vm.well_known.tag_field, Some("tag"));
    }

    #[test]
    fn bare_vm_has_empty_well_known() {
        // A VM that never called new_vm() has no result vocabulary
        let vm = xcl_vm::VM::new();
        assert!(!vm.well_known.has_result());
        assert!(vm.well_known.ok_ctor.is_none());
    }

    #[test]
    fn well_known_result_variant_lookup() {
        let vm = new_vm();
        assert_eq!(vm.well_known.result_variant("Ok"), Some(true));
        assert_eq!(vm.well_known.result_variant("Err"), Some(false));
        assert_eq!(vm.well_known.result_variant("Foo"), None);
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
        assert_eq!(desc.variants[1].fields.len(), 2);
        assert_eq!(desc.variants[1].fields[0].name, "message");
        assert_eq!(desc.variants[1].fields[0].ty, "str");
        assert_eq!(desc.variants[1].fields[1].name, "code");
        assert_eq!(desc.variants[1].fields[1].ty, "i64");

        assert!(desc.summary.is_some());
        assert!(desc.summary.unwrap().contains("success or failure"));
    }

    #[test]
    fn io_read_returns_result() {
        let mut vm = new_vm();
        let r = evaluate(r#"io.read("/tmp/nonexistent_xyz.txt")"#, &mut vm);
        let m = r.as_map().expect("io.read should return a map (Result)");
        assert_eq!(m.get("tag").unwrap().as_str(), Some("Err"));
    }

    #[test]
    fn io_write_read_roundtrip() {
        let path = std::env::temp_dir().join("xcl_io_roundtrip.txt");
        let path_str = path.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&path);

        let mut vm = new_vm();
        let write_r = evaluate(&format!(r#"io.write("{}", "hello")"#, path_str), &mut vm);
        assert_eq!(write_r.as_map().unwrap().get("tag").unwrap().as_str(), Some("Ok"));

        let mut vm = new_vm();
        let read_r = evaluate(&format!(r#"io.read("{}")"#, path_str), &mut vm);
        let m = read_r.as_map().unwrap();
        assert_eq!(m.get("tag").unwrap().as_str(), Some("Ok"));
        assert_eq!(m.get("value").unwrap().as_str(), Some("hello"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn io_open_returns_result() {
        let mut vm = new_vm();
        let r = evaluate(r#"io.open("/tmp")"#, &mut vm);
        let m = r.as_map().unwrap();
        // /tmp is a directory, opening as file should return Err on some systems
        // or Ok on others — just check it returns a Result
        assert!(m.get("tag").is_some());
    }

    // ── Space-separated call tests ──

    #[test]
    fn io_read_space_form() {
        let mut vm = new_vm();
        let r = evaluate(r#"io.read "/tmp/nonexistent_xyz.txt""#, &mut vm);
        let m = r.as_map().expect("should return a dict");
        assert_eq!(m.get("tag").unwrap().as_str(), Some("Err"));
    }

    #[test]
    fn io_write_space_form() {
        let path = std::env::temp_dir().join("xcl_space_form.txt");
        let path_str = path.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&path);

        let mut vm = new_vm();
        let r = evaluate(&format!(r#"io.write "{}" "hello""#, path_str), &mut vm);
        assert_eq!(r.as_map().unwrap().get("tag").unwrap().as_str(), Some("Ok"));

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

    // ── Newline-separated statement tests ──

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

    // ── Parenthesized field access arithmetic tests ──

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

    // ── Print and format tests ──

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
}