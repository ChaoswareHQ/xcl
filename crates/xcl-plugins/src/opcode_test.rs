//! End-to-end tests for extension opcodes: hot bytes and cold modules.
//!
//! The hot tier is one instruction per byte in `128..=255`. The cold tier is
//! reached through `OP_EXT` (127): operand `a` names the module, `c` names the
//! instruction inside it, `b` is a 16-bit operand.

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use xcl_macros::opcode;
    use xcl_vm::{Bytecode, Instr, OpcodeResult, Value};

    const OP_EXT: u8 = 127;
    const OP_HALT: u8 = 34;

    // ── Hot tier: one byte, one instruction ──

    #[opcode(name = "OP_XCL_TEST", code = 200)]
    fn op_xcl_test(vm: &mut xcl_vm::VM, _top: usize, _instr: Instr) -> OpcodeResult {
        vm.push_value(Value::from_i48(42));
        OpcodeResult::Next
    }

    // ── Cold tier: module 7, two instructions ──

    fn cold_double(vm: &mut xcl_vm::VM, _top: usize, instr: Instr) -> OpcodeResult {
        vm.push_value(Value::from_i48(instr.b() as i64 * 2));
        OpcodeResult::Next
    }

    fn cold_name(vm: &mut xcl_vm::VM, _top: usize, _instr: Instr) -> OpcodeResult {
        vm.push_value(Value::from_i48(7));
        OpcodeResult::Next
    }

    crate::cold_module! {
        XCL_TEST_COLD = { id: 7, name: "xcl_test", ops: {
            0 => ("OP_TEST_DOUBLE", cold_double),
            1 => ("OP_TEST_NAME", cold_name),
        }}
    }

    fn run(instrs: Vec<Instr>) -> Value {
        let mut vm = crate::new_vm();
        let bc = Bytecode {
            instructions: Arc::from(instrs),
            wide_operands: Arc::new([]),
            string_table: Arc::new([]),
            string_values: Arc::new([]),
            local_names: Arc::new([]),
            global_names: Arc::new([]),
            user_functions: Vec::new().into(),
            closures: Vec::new().into(),
        };
        let result = vm.run(&bc);
        core::mem::forget(vm);
        result
    }

    #[test]
    fn hot_byte_opcode_runs() {
        let v = run(vec![Instr::a1(OP_XCL_TEST, 0), Instr::none(OP_HALT)]);
        assert_eq!(v.as_i48(), 42);
    }

    #[test]
    fn cold_module_opcode_runs_with_its_operand() {
        // OP_EXT(a = 7, b = 21, c = 0) -> local 0 -> doubles b.
        let v = run(vec![
            Instr::a2(OP_EXT, 7, 21).with_c(0),
            Instr::none(OP_HALT),
        ]);
        assert_eq!(v.as_i48(), 42);
    }

    #[test]
    fn cold_module_selects_by_local_opcode() {
        let v = run(vec![
            Instr::a2(OP_EXT, 7, 0).with_c(1),
            Instr::none(OP_HALT),
        ]);
        assert_eq!(v.as_i48(), 7);
    }

    #[test]
    fn unknown_cold_module_is_a_no_op() {
        // Module 99 is not registered; execution continues to the halt.
        let v = run(vec![
            Instr::a2(OP_EXT, 99, 21).with_c(0),
            Instr::a1(OP_XCL_TEST, 0),
            Instr::none(OP_HALT),
        ]);
        assert_eq!(v.as_i48(), 42);
    }

    #[test]
    fn unknown_cold_opcode_is_a_no_op() {
        let v = run(vec![
            Instr::a2(OP_EXT, 7, 21).with_c(200),
            Instr::a1(OP_XCL_TEST, 0),
            Instr::none(OP_HALT),
        ]);
        assert_eq!(v.as_i48(), 42);
    }

    #[test]
    fn hot_and_cold_tiers_coexist() {
        let v = run(vec![
            Instr::a1(OP_XCL_TEST, 0),
            Instr::a2(OP_EXT, 7, 4).with_c(0),
            Instr::none(OP_HALT),
        ]);
        // The hot op pushes 42, then the cold op pushes 4 * 2, which is on top.
        assert_eq!(v.as_i48(), 8);
    }

    #[test]
    fn std_opcodes_land_in_the_std_registry() {
        // `#[opcode]` picks the registry from `CARGO_PKG_NAME`; this file is in
        // `xcl-plugins`, so the entry must be in the std slice and not the
        // plugin one. That is what makes "std loads first" automatic.
        assert!(
            crate::STD_OPCODE_REGISTRY
                .iter()
                .any(|e| e.code == OP_XCL_TEST)
        );
        assert!(!crate::OPCODE_REGISTRY.iter().any(|e| e.code == OP_XCL_TEST));
        assert_eq!(
            crate::registered_opcode_name(OP_XCL_TEST),
            Some("OP_XCL_TEST")
        );
    }

    #[test]
    fn cold_module_is_registered() {
        let module = crate::COLD_MODULE_REGISTRY
            .iter()
            .find(|m| m.id == 7)
            .expect("xcl_test module is registered");
        assert_eq!(module.name, "xcl_test");
        assert_eq!(module.ops.len(), 2);
        assert_eq!(module.ops[0].name, "OP_TEST_DOUBLE");
    }
}
