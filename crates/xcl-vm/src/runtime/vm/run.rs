//! The dispatch loop.
//!
//! `ip` and the frame's code slice live in locals, not behind `frames[top]`:
//! every instruction needs both, so the loop writes them back to the frame only
//! when a call is entered. `JIT == false` contains no JIT code at all.

#[cfg(feature = "jit")]
use super::jit::{JIT_MAX_LOOPS, fill_loop_cache};
use super::*;

impl VM {
    /// Dispatch loop, monomorphised on whether a native loop is installed.
    ///
    /// `JIT == false` contains no JIT code at all, so interpreted programs pay
    /// nothing for the feature being compiled in.
    pub(super) fn run_inner<const JIT: bool>(&mut self, bc: &Bytecode, jit: JitPlan<'_>) -> Value {
        #[cfg(feature = "jit")]
        let mut jit = jit;
        #[cfg(not(feature = "jit"))]
        let _ = jit;
        self.stack.clear();
        self.frame_depth = 0;
        #[cfg(feature = "trace")]
        self.trace.clear();
        if self.frames.len() > 4096 {
            self.frames.truncate(4096);
        }
        // Clear per-run state so repeated runs leave nothing behind.
        // Only the callee-cache slots a previous run actually wrote are cleared;
        // the rest are already nil.
        self.context_stack.clear();
        while self.call_cache_dirty != 0 {
            let slot = self.call_cache_dirty.trailing_zeros() as usize;
            self.call_cache_dirty &= self.call_cache_dirty - 1;
            self.call_cache[slot] = (Value::nil(), Value::nil());
            #[cfg(feature = "jit")]
            {
                self.jit_call_cache[slot] = None;
            }
        }
        self.arena.reset();
        // `$0` is materialised on first read (`op_special_var`). Building it here
        // would force the arena's first chunk for programs that never mention `$0`.
        self.script_name_value = Value::nil();

        // Install (or refresh) the bytecode's top-level functions.
        //
        // Function values live in the arena, so they must be rebuilt after the
        // arena reset above. They are not inserted into `global_index`: a named
        // call resolves through `Bytecode::function_by_name`, which is one array
        // load, and the slots are the contiguous range starting at `function_base`.
        let current_code = bc.instructions.clone();
        let is_same_bytecode = self
            .installed_code
            .as_ref()
            .map(|prev| Arc::ptr_eq(prev, &current_code))
            .unwrap_or(false);

        if !is_same_bytecode {
            // Drop the previous bytecode's function values.
            self.globals.truncate(self.function_base as usize);
        }

        // Compiler globals (enum variants, type names) occupy slots
        // `0..global_names.len()` and come before the function values.
        let compiler_globals = bc.global_names.len();
        if self.globals.len() < compiler_globals {
            self.globals.resize(compiler_globals, Value::nil());
        }

        if !is_same_bytecode {
            let function_base = self.globals.len() as u32;
            self.globals.reserve(bc.user_functions.len());
            for (_name, func) in bc.user_functions.iter() {
                self.globals
                    .push(Value::from_func_shared(Arc::clone(func), &self.arena));
            }
            self.function_base = function_base;
            self.installed_code = Some(current_code);
        } else {
            // Same bytecode — rewrite each slot with a fresh Value (arena was reset).
            let base = self.function_base as usize;
            for (i, (_name, func)) in bc.user_functions.iter().enumerate() {
                self.globals[base + i] = Value::from_func_shared(Arc::clone(func), &self.arena);
            }
        }

        let n = bc.local_names.len().max(16);
        self.regs.clear();
        // SAFETY: bc is borrowed by run() and lives for the entire call.
        let root = self.acquire_frame(bc);
        // The root frame has no parameters, so every slot is nil-filled.
        self.bind_registers(root, n, 0);
        {
            let frame = &mut self.frames[root];
            frame.stack_base = 0;
            frame.argv.extend_from_slice(&self.script_args);
        }

        // Dispatch state kept in locals instead of behind `frames[top]`.
        //
        // The instruction pointer and the current code slice are the two things
        // every instruction needs. The previous loop reloaded `frames[top]`,
        // `frame.ip` and `frame.code.instructions` (three dependent loads) and
        // wrote `ip` back through the frame for every single instruction.
        // Now `ip` lives in a register and is only written to the frame when a
        // call is entered (the return address), which is the only time another
        // frame needs it.
        let mut top = self.frame_count() - 1;
        let mut ip: usize = 0;
        let (mut code_ptr, mut code_len) = frame_code_bounds(&self.frames[top]);
        let mut locals = self.frames[top].locals;

        // Native loop state. The shadow table maps an instruction to its
        // compiled loop; `jit_disabled` remembers loops that bailed out during
        // this run. It applies only to the root frame's own bytecode. With
        // `JIT == false` every use of these is eliminated.
        #[cfg(feature = "jit")]
        let (jit_shadow_ptr, jit_shadow_len) = match jit.as_deref() {
            Some(u) => (u.shadow.as_ptr(), u.shadow.len()),
            None => (core::ptr::null(), 0),
        };
        #[cfg(feature = "jit")]
        let mut jit_disabled: u64 = 0;
        #[cfg(feature = "jit")]
        let root_code_ptr = bc.instructions.as_ptr();

        loop {
            if ip >= code_len {
                // Run deferred closures before returning
                self.run_defers();
                let r = self.stack.pop().unwrap_or(Value::nil());
                let frame = unsafe { self.frames.get_unchecked(top) };
                let returning_base = frame.stack_base;
                let local_base = frame.local_base as usize;
                self.regs.truncate(local_base);
                self.frame_depth -= 1;
                debug_assert_eq!(self.frame_depth, top);
                if self.frame_depth == 0 {
                    return Value::escape(r);
                }
                top -= 1;
                let caller = unsafe { self.frames.get_unchecked(top) };
                let base = returning_base;
                ip = caller.ip as usize;
                (code_ptr, code_len) = frame_code_bounds(caller);
                locals = caller.locals;
                self.stack.truncate(base as usize);
                self.stack.push(r);
                continue;
            }

            let instr = unsafe { *code_ptr.add(ip) };

            // Trace hook: record which instruction this frame is about to run.
            // Compiled out entirely without the `trace` feature.
            #[cfg(feature = "trace")]
            if self.trace.len() < Self::TRACE_CAP {
                self.trace.push((ip as u32, top as u8, instr.op()));
            }

            // Enter a native loop when the root frame reaches a compiled
            // header. The check is gated on the loop-header opcode, so
            // interpreted instructions pay one compare rather than a shadow
            // load. A bailout (non-integer local, multiply overflow, division
            // by zero) leaves the register file untouched, so the interpreter
            // re-executes the loop; that loop is disabled for the rest of this
            // run. In the `JIT == false` instantiation this block is gone.
            #[cfg(feature = "jit")]
            if JIT && top == 0 && code_ptr == root_code_ptr && jit_shadow_len != 0 {
                let op = instr.op();
                if op == ops::consts::OP_WHILE_LT_INT || op == ops::consts::OP_WHILE_LT_INT_WIDE {
                    // SAFETY: `ip < code_len`, and the shadow belongs to the
                    // root bytecode, so its length is `code_len`.
                    let slot = unsafe { *jit_shadow_ptr.add(ip) };
                    let idx = slot.wrapping_sub(1) as usize;
                    if slot != 0 && idx < JIT_MAX_LOOPS && jit_disabled & (1u64 << idx) == 0 {
                        if let Some(unit) = jit.as_deref_mut() {
                            let entry = unit.entries[idx];
                            let base = self.frames[top].local_base as usize;
                            if !fill_loop_cache(&self.regs, bc, unit, idx, base) {
                                jit_disabled |= 1u64 << idx;
                            } else {
                                let ptr = unsafe { self.regs.as_mut_ptr().add(base) }.cast::<u64>();
                                // SAFETY: `ptr` addresses the root frame's registers,
                                // the frame is active, and the JIT only touches its locals.
                                let cache_ptr: *mut xcl_jit::LoopCache = &raw mut unit.caches[idx];
                                let rc = unsafe { (entry.func)(ptr, cache_ptr) };
                                if rc == 0 {
                                    ip = entry.exit_ip;
                                    continue;
                                }
                                jit_disabled |= 1u64 << idx;
                            }
                        }
                    }
                }
            }

            let action = self.dispatch(locals, top, instr);
            match action {
                Action::Next => ip += 1,
                Action::Jump(to) => ip = to as usize,
                Action::TailCall => {
                    // The frame rebound to another function's bytecode.
                    ip = 0;
                    let frame = unsafe { self.frames.get_unchecked(top) };
                    (code_ptr, code_len) = frame_code_bounds(frame);
                    locals = frame.locals;
                }
                Action::Return => {
                    // Run deferred closures before returning
                    self.run_defers();
                    let r = self.stack.pop().unwrap_or(Value::nil());
                    let frame = unsafe { self.frames.get_unchecked(top) };
                    let returning_base = frame.stack_base;
                    let local_base = frame.local_base as usize;
                    self.regs.truncate(local_base);
                    self.frame_depth -= 1;
                    if self.frame_depth == 0 {
                        return Value::escape(r);
                    }
                    top -= 1;
                    let caller = unsafe { self.frames.get_unchecked(top) };
                    let base = returning_base;
                    ip = caller.ip as usize;
                    (code_ptr, code_len) = frame_code_bounds(caller);
                    locals = caller.locals;
                    self.stack.truncate(base as usize);
                    self.stack.push(r);
                }
                Action::PushFrame => {
                    // The callee frame is already on the stack. Record where the
                    // caller resumes; nothing else needs the caller's ip.
                    unsafe { self.frames.get_unchecked_mut(top) }.ip = (ip + 1) as u32;
                    top += 1;
                    ip = 0;
                    let frame = unsafe { self.frames.get_unchecked(top) };
                    (code_ptr, code_len) = frame_code_bounds(frame);
                    locals = frame.locals;
                }
            }
        }
    }

    #[inline(always)]
    pub(super) fn dispatch(&mut self, locals: LocalSlice, top: usize, i: Instr) -> Action {
        #[cfg(feature = "opcode-stats")]
        self.opcode_counter.record(i.op());

        let op = i.op();
        // The space is partitioned by one bit (`opcode::layout`), so a module-space
        // instruction goes straight to its handler instead of paying a call into
        // `dispatch_core` that can only return `None`. Core-only programs never
        // take this branch, so the check costs a predicted-not-taken test.
        if op & crate::runtime::opcode::layout::MODULE_SPACE_MASK != 0 {
            let Some(table) = self.opcode_table.as_ref() else {
                // No plugin opcodes registered: an unclaimed slot is a no-op,
                // exactly like the empty table this replaces.
                return Action::Next;
            };
            return match table[crate::runtime::opcode::layout::module_index(op) as usize] {
                Some(handler) => handler(self, top, i),
                None => Action::Next,
            };
        }

        // Core opcode dispatch. Only module-space codes can own a handler (see
        // `register_opcodes`), so an unhandled code here is `OP_EXT` — reserved,
        // not implemented — or an unclaimed growth slot.
        match ops::dispatch_core(self, locals, top, i) {
            Some(result) => result,
            None => Action::Next,
        }
    }
}
