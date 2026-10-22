//! Calls: entering a callee, tail calls, and reentrant `call_value`.
//!
//! A call takes its arguments from the operand stack, binds the callee frame
//! in place, and never moves a `Frame`. The variants differ only in where the
//! arguments come from: a slice, the stack at `base`, or the stack below the
//! fused `OP_CALL_NAME` instruction.

use super::*;

impl VM {
    /// Runs the top frame's deferred closures in LIFO order.
    ///
    /// Returns immediately when there is nothing deferred, which is the common
    /// case; every function return goes through here.
    #[inline(always)]
    pub(super) fn run_defers(&mut self) {
        let Some(frame) = self.top_frame_mut() else {
            return;
        };
        if frame.defer_stack.is_empty() {
            return;
        }
        let mut stack = core::mem::take(&mut frame.defer_stack);
        stack.reverse();
        for closure in stack {
            let _ = self.call_value(closure, &[]);
        }
    }

    pub(super) fn enter_call(&mut self, func: &crate::runtime::value::FuncValue, args: &[Value]) -> Action {
        let Some(bc) = &func.bytecode else {
            self.push(Value::nil());
            return Action::Next;
        };

        let n = bc.local_names.len().max(func.params.len());
        let base = self.stack.len() as u32;

        let idx = self.acquire_frame(bc);
        let copy = func.params.len().min(args.len());
        let reg_base = self.bind_registers(idx, n, copy);
        self.regs[reg_base..reg_base + copy].copy_from_slice(&args[..copy]);
        self.install_captures(reg_base, n, func);

        let frame = &mut self.frames[idx];
        frame.ip = 0;
        frame.stack_base = base;
        frame.arg0 = args.first().copied().unwrap_or(Value::nil());

        // Prepare argv only when the body actually reads `$1..$#` / `$@` / `$*`.
        if func.uses_argv {
            frame.argv.extend_from_slice(args);
        }

        // Return PushFrame so the dispatch loop doesn't advance ip
        Action::PushFrame
    }

    /// Enters a call whose callee and arguments are still on the operand stack.
    ///
    /// `arg_base` is the callee's slot; the arguments occupy
    /// `arg_base + 1 .. arg_base + 1 + nargs`. The operand stack is truncated to
    /// `arg_base` so the return value lands exactly where the callee was.
    ///
    /// This avoids materializing an intermediate argument buffer for the common
    /// case of calling a script function.
    pub(super) fn enter_call_at(
        &mut self,
        func: &crate::runtime::value::FuncValue,
        arg_base: usize,
        nargs: usize,
    ) -> crate::runtime::opcode::OpcodeResult {
        let Some(bc) = &func.bytecode else {
            self.stack.truncate(arg_base);
            self.push(Value::nil());
            return crate::runtime::opcode::OpcodeResult::Next;
        };

        let n = bc.local_names.len().max(func.params.len());
        let arg_start = arg_base + 1;

        let idx = self.acquire_frame(bc);
        let copy = func.params.len().min(nargs);
        let reg_base = self.bind_registers(idx, n, copy);
        self.regs[reg_base..reg_base + copy]
            .copy_from_slice(&self.stack[arg_start..arg_start + copy]);
        self.install_captures(reg_base, n, func);

        {
            let frame = &mut self.frames[idx];
            frame.ip = 0;
            frame.stack_base = arg_start as u32;
            frame.arg0 = self.stack.get(arg_start).copied().unwrap_or(Value::nil());

            if func.uses_argv {
                frame
                    .argv
                    .extend_from_slice(&self.stack[arg_start..arg_start + nargs]);
            }
        }

        self.stack.truncate(arg_base);
        crate::runtime::opcode::OpcodeResult::PushFrame
    }

    /// Enters a call whose arguments are on the operand stack but whose callee
    /// is **not** (fused named calls embed it in the instruction).
    ///
    /// `arg_start` is the first argument; the result replaces it on return.
    pub(super) fn enter_call_args_at(
        &mut self,
        func: &crate::runtime::value::FuncValue,
        arg_start: usize,
        nargs: usize,
    ) -> crate::runtime::opcode::OpcodeResult {
        let Some(bc) = &func.bytecode else {
            self.stack.truncate(arg_start);
            self.push(Value::nil());
            return crate::runtime::opcode::OpcodeResult::Next;
        };

        let n = bc.local_names.len().max(func.params.len());
        let copy = func.params.len().min(nargs);

        let idx = self.acquire_frame(bc);
        let reg_base = self.bind_registers(idx, n, copy);
        self.regs[reg_base..reg_base + copy]
            .copy_from_slice(&self.stack[arg_start..arg_start + copy]);
        self.install_captures(reg_base, n, func);

        {
            let frame = &mut self.frames[idx];
            frame.ip = 0;
            frame.stack_base = arg_start as u32;
            frame.arg0 = self.stack.get(arg_start).copied().unwrap_or(Value::nil());

            if func.uses_argv {
                frame
                    .argv
                    .extend_from_slice(&self.stack[arg_start..arg_start + nargs]);
            }
        }

        self.stack.truncate(arg_start);
        crate::runtime::opcode::OpcodeResult::PushFrame
    }

    /// Same as `enter_call` but returns [`OpcodeResult`] for use from opcode handlers.
    pub(super) fn enter_call_from_op(
        &mut self,
        func: &crate::runtime::value::FuncValue,
        args: &[Value],
    ) -> crate::runtime::opcode::OpcodeResult {
        match self.enter_call(func, args) {
            Action::PushFrame => crate::runtime::opcode::OpcodeResult::PushFrame,
            Action::Next => crate::runtime::opcode::OpcodeResult::Next,
            _ => crate::runtime::opcode::OpcodeResult::Next,
        }
    }

    /// Tail-call entry: reuses the current frame instead of pushing a new one.
    pub(super) fn enter_tail_call(
        &mut self,
        func: &crate::runtime::value::FuncValue,
        args: &[Value],
    ) -> crate::runtime::opcode::OpcodeResult {
        let Some(bc) = &func.bytecode else {
            if let Some(frame) = self.top_frame() {
                self.stack.truncate(frame.stack_base as usize);
            }
            self.push(Value::nil());
            return crate::runtime::opcode::OpcodeResult::Next;
        };

        let n = bc.local_names.len().max(func.params.len());

        // Reuse the current frame instead of pushing a new one
        let top = match self.frame_depth.checked_sub(1) {
            Some(i) => i,
            None => return crate::runtime::opcode::OpcodeResult::Next,
        };
        let frame = &mut self.frames[top];

        // SAFETY: this frame is active (index < frame_depth) during run().
        frame.code_ptr = Arc::as_ptr(bc);
        frame.ip = 0;
        frame.iter.clear();
        frame.iter_idx = 0;
        frame.iter_is_range = false;
        frame.range_cur = 0;
        frame.range_end = 0;
        frame.defer_stack.clear();

        // Rebind the frame's registers (the tail-called function may have a
        // different local count) and write the new arguments.
        let base = frame.local_base as usize;
        let copy = func.params.len().min(args.len());
        self.rebind_registers(top, base, n, copy);
        self.regs[base..base + copy].copy_from_slice(&args[..copy]);
        self.install_captures(base, n, func);

        // Prepare argv only when the body actually reads `$1..$#` / `$@` / `$*`.
        let frame = &mut self.frames[top];
        frame.argv.clear();
        frame.arg0 = args.first().copied().unwrap_or(Value::nil());
        if func.uses_argv {
            frame.argv.extend_from_slice(args);
        }

        // Truncate operand stack to this frame's base
        self.stack.truncate(frame.stack_base as usize);

        // Jump to ip=0 so the dispatch loop starts the new code from the beginning
        crate::runtime::opcode::OpcodeResult::TailCall
    }

    /// Reentrantly invokes an XCL function or closure value with the given arguments.
    ///
    /// Runs the VM dispatch loop until the called function's frame returns, then returns its result.
    ///
    /// # Arguments
    ///
    /// * `func` - The [`Value`] representing the function or closure to invoke.
    /// * `args` - Positional and named arguments passed to the function.
    ///
    /// # Returns
    ///
    /// The [`Value`] returned by the invoked function, or `nil` if `func` is not callable.
    pub fn call_value(&mut self, func: Value, args: &[Value]) -> Value {
        let func_ref = match func.as_func() {
            Some(f) => f,
            None => return Value::nil(),
        };

        let bc = match &func_ref.bytecode {
            Some(b) => Arc::clone(b),
            None => return Value::nil(),
        };

        // Prepare locals: params first (matching how enter_call does it)
        let n = bc.local_names.len().max(func_ref.params.len());

        let initial_depth = self.frame_depth;
        let initial_regs = self.regs.len();
        let stack_base = self.stack.len() as u32;

        let idx = self.acquire_frame(&bc);
        let copy = func_ref.params.len().min(args.len());
        let reg_base = self.bind_registers(idx, n, copy);
        self.regs[reg_base..reg_base + copy].copy_from_slice(&args[..copy]);
        self.install_captures(reg_base, n, func_ref);
        {
            let frame = &mut self.frames[idx];
            frame.stack_base = stack_base;
            frame.argv.extend_from_slice(args);
        }

        // Run the VM dispatch loop until the frame we just pushed returns
        loop {
            // Recalculate top every iteration because PushFrame/Return change frame count
            let top = match self.frame_depth.checked_sub(1) {
                Some(i) => i,
                None => {
                    self.frame_depth = initial_depth;
                    self.regs.truncate(initial_regs);
                    return Value::nil();
                }
            };

            // If we've returned to the caller depth, the closure is done
            if self.frame_depth <= initial_depth {
                break;
            }

            let frame = &self.frames[top];
            // SAFETY: frame is active (index < frame_depth) during call_value().
            if frame.ip as usize >= frame.code().instructions.len() {
                // Run deferred closures before returning
                self.run_defers();
                // Frame ran off the end — treat top of stack as return value
                let result = self.stack.pop().unwrap_or(Value::nil());
                let returning_base = self.top_frame().map(|f| f.stack_base);
                self.release_registers(top);
                self.frame_depth -= 1;
                if self.frame_depth <= initial_depth {
                    self.stack
                        .truncate(returning_base.unwrap_or(stack_base) as usize);
                    return Value::escape(result);
                }
                // Nested call returned; hand the value to the outer frame
                let caller = &mut self.frames[self.frame_depth - 1];
                caller.ip += 1;
                let base = returning_base.unwrap_or(caller.stack_base);
                self.stack.truncate(base as usize);
                self.stack.push(result);
                continue;
            }

            // SAFETY: frame is active (index < frame_depth) during call_value().
            let instr = unsafe { *frame.code().instructions.get_unchecked(frame.ip as usize) };
            let action = self.dispatch(frame.locals, top, instr);
            match action {
                Action::Next => {
                    let top = self.frame_depth.checked_sub(1).unwrap_or(top);
                    self.frames[top].ip += 1;
                }
                Action::Jump(to) => {
                    let top = self.frame_depth.checked_sub(1).unwrap_or(top);
                    self.frames[top].ip = to;
                }
                Action::TailCall => {
                    let top = self.frame_depth.checked_sub(1).unwrap_or(top);
                    self.frames[top].ip = 0;
                }
                Action::Return => {
                    // Run deferred closures before returning
                    self.run_defers();
                    let result = self.stack.pop().unwrap_or(Value::nil());
                    let returning_base = self.top_frame().map(|f| f.stack_base);
                    self.release_registers(top);
                    self.frame_depth -= 1;
                    if self.frame_depth <= initial_depth {
                        self.stack
                            .truncate(returning_base.unwrap_or(stack_base) as usize);
                        return Value::escape(result);
                    }
                    if let Some(caller) = self.frames.get_mut(self.frame_depth - 1) {
                        caller.ip += 1;
                    }
                    let caller_base = self.frames[self.frame_depth - 1].stack_base;
                    self.stack
                        .truncate(returning_base.unwrap_or(caller_base) as usize);
                    self.stack.push(result);
                }
                Action::PushFrame => {
                    // Frame already pushed by enter_call; just continue
                    continue;
                }
            }
        }

        self.regs.truncate(initial_regs);
        Value::nil()
    }
}
