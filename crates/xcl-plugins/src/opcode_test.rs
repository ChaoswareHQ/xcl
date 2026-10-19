//! End-to-end test for registerable opcodes.

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use xcl_macros::xcl_opcode;
    use xcl_vm::{Instr, OpcodeResult, Value};

    #[xcl_opcode(name = "OP_XCL_TEST", code = 200)]
    fn op_xcl_test(vm: &mut xcl_vm::VM, _top: usize, _instr: Instr) -> OpcodeResult {
        vm.push_value(Value::from_i48(42));
        OpcodeResult::Next
    }

    #[test]
    fn test_custom_opcode_returns_42() {
        let mut vm = crate::new_vm();

        // Build minimal bytecode: OP_XCL_TEST followed by OP_HALT(34)
        let instrs: Arc<[Instr]> = Arc::from(vec![Instr::a1(OP_XCL_TEST, 0), Instr::none(34)]);

        let bc = xcl_vm::Bytecode {
            instructions: instrs,
            wide_operands: Arc::new([]),
            string_table: Arc::new([]),
            string_values: Arc::new([]),
            local_names: Arc::new([]),
            global_names: Arc::new([]),
            user_functions: Vec::new().into(),
            closures: Vec::new().into(),
        };

        let result = vm.run(&bc);
        assert!(result.is_int(), "expected int, got {:?}", result);
        assert_eq!(result.as_i48(), 42);
    }
}
