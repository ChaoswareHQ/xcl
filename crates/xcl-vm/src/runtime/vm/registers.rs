//! Register-file management.
//!
//! Locals live in one contiguous `VM::regs` array; a frame owns the slice
//! `[local_base, local_base + len)`. Entering a call reserves slots at the top
//! and binds the frame to them; returning truncates. Frames cache a raw pointer
//! into the file, so growing it must rebase every live frame.

use super::*;

impl VM {
    /// Number of live call frames.
    #[inline(always)]
    pub(super) fn frame_count(&self) -> usize {
        self.frame_depth
    }

    /// The live frame at the top of the call stack, if any.
    #[inline(always)]
    pub(super) fn top_frame(&self) -> Option<&Frame> {
        match self.frame_depth {
            0 => None,
            n => Some(unsafe { self.frames.get_unchecked(n - 1) }),
        }
    }

    /// Mutable access to the live frame at the top of the call stack.
    #[inline(always)]
    pub(super) fn top_frame_mut(&mut self) -> Option<&mut Frame> {
        match self.frame_depth {
            0 => None,
            n => Some(unsafe { self.frames.get_unchecked_mut(n - 1) }),
        }
    }

    /// Acquires the frame slot for a new call and returns its index.
    ///
    /// The slot is reset in place, so no `Frame` is ever moved. Callers then
    /// fill in `stack_base`, `locals` and `argv` through `self.frames[idx]`.
    #[inline]
    pub(super) fn acquire_frame(&mut self, bc: &Bytecode) -> usize {
        let idx = self.frame_depth;
        if idx == self.frames.len() {
            self.frames.push(Frame {
                code_ptr: bc as *const Bytecode,
                locals: LocalSlice::EMPTY,
                local_base: 0,
                ip: 0,
                iter: Vec::new(),
                iter_idx: 0,
                iter_is_range: false,
                range_cur: 0,
                range_end: 0,
                stack_base: 0,
                defer_stack: Vec::new(),
                arg0: Value::nil(),
                argv: Vec::new(),
            });
        }
        self.frame_depth = idx + 1;

        let frame = unsafe { self.frames.get_unchecked_mut(idx) };
        frame.code_ptr = bc as *const Bytecode;
        // `ip` is written by the caller once it knows the return address.
        // Iterator state is initialized by the `OP_FOR_*_START` opcodes before
        // it is read, so a call does not clear it; only the buffers that ops
        // push into must start empty.
        if !frame.defer_stack.is_empty() {
            frame.defer_stack.clear();
        }
        // `argv` is cleared by the call entry that fills it, so only the calls
        // that actually read `$1..$N` pay for it.
        // `locals`/`local_base` are set by `bind_registers`, which every caller
        // runs next.
        idx
    }

    /// Allocates `n` registers for frame `idx` at the top of the register file
    /// and binds the frame to them.
    ///
    /// `copied_params` is the number of slots the caller writes immediately
    /// after this call (parameters taken from the operand stack). Those slots
    /// are not written here, which removes one store per parameter from the
    /// call path. The remaining `[copied_params..n)` slots are nil-filled.
    /// A count of `0` (the root frame) nil-fills everything.
    #[inline(always)]
    // Clippy cannot see that every element is written before it is read; the
    // SAFETY comment below is the proof. `Value` is `Copy` and has no `Drop`.
    #[allow(clippy::uninit_vec)]
    pub(super) fn bind_registers(&mut self, idx: usize, n: usize, copied_params: usize) -> usize {
        let base = self.regs.len();
        let old_ptr = self.regs.as_ptr();
        self.regs.reserve(n);
        // Active frames cache a raw pointer into the register file. If the
        // store grew, every cached pointer now dangles and must be rebased
        // before any of those frames runs again (e.g. after a callee returns).
        if self.regs.as_ptr() != old_ptr {
            self.rebase_frames(idx);
        }
        // SAFETY: `Value` is `Copy` and has no `Drop`. `reserve` guarantees
        // capacity for `base + n`. Every slot in `[base, base + n)` is written
        // before it can be read: the caller writes
        // `[0, copied_params)` right after this call, and the loop below
        // writes the rest. `write` does not read the old (possibly
        // uninitialised) contents.
        unsafe {
            self.regs.set_len(base + n);
            let start = self.regs.as_mut_ptr().add(base + copied_params.min(n));
            let end = self.regs.as_mut_ptr().add(base + n);
            let mut p = start;
            while p < end {
                p.write(Value::nil());
                p = p.add(1);
            }
        }
        let frame = unsafe { self.frames.get_unchecked_mut(idx) };
        frame.local_base = base as u32;
        frame.locals = LocalSlice {
            ptr: unsafe { self.regs.as_mut_ptr().add(base) },
            len: n as u32,
        };
        base
    }

    /// Rebinds every frame below `active` to the current register allocation.
    ///
    /// Called only when growing `regs` moved the buffer; `LocalSlice` stores a
    /// raw pointer, so stale pointers make a returning frame read or write the
    /// freed block (the hanoi benchmark crosses the initial 64-slot capacity).
    #[inline]
    pub(super) fn rebase_frames(&mut self, active: usize) {
        let base_ptr = self.regs.as_mut_ptr();
        for i in 0..active {
            let frame = unsafe { self.frames.get_unchecked_mut(i) };
            frame.locals = LocalSlice {
                ptr: unsafe { base_ptr.add(frame.local_base as usize) },
                len: frame.locals.len,
            };
        }
    }

    /// Drops the registers of a frame that is returning.
    #[inline(always)]
    pub(super) fn release_registers(&mut self, idx: usize) {
        let base = unsafe { self.frames.get_unchecked(idx) }.local_base as usize;
        self.regs.truncate(base);
    }

    /// Rebinds frame `idx` to `n` registers at `base`, growing the file if the
    /// (tail-called) function needs more slots than the current one.
    ///
    /// `copied_params` slots are overwritten by the caller; the tail is
    /// nil-filled.
    #[inline(always)]
    #[allow(clippy::uninit_vec)]
    pub(super) fn rebind_registers(
        &mut self,
        idx: usize,
        base: usize,
        n: usize,
        copied_params: usize,
    ) {
        self.regs.truncate(base);
        let old_ptr = self.regs.as_ptr();
        self.regs.reserve(n);
        if self.regs.as_ptr() != old_ptr {
            self.rebase_frames(idx);
        }
        // SAFETY: same argument as `bind_registers`: `Value` has no `Drop`, the
        // capacity is reserved, and every slot is written before it is read.
        unsafe {
            self.regs.set_len(base + n);
            let start = self.regs.as_mut_ptr().add(base + copied_params.min(n));
            let end = self.regs.as_mut_ptr().add(base + n);
            let mut p = start;
            while p < end {
                p.write(Value::nil());
                p = p.add(1);
            }
        }
        let frame = unsafe { self.frames.get_unchecked_mut(idx) };
        frame.local_base = base as u32;
        frame.locals = LocalSlice {
            ptr: unsafe { self.regs.as_mut_ptr().add(base) },
            len: n as u32,
        };
    }

    /// Writes a closure's captured values into the callee frame's capture
    /// slots, which sit immediately after its parameters.
    #[inline]
    pub(super) fn install_captures(
        &mut self,
        reg_base: usize,
        n: usize,
        func: &crate::runtime::value::FuncValue,
    ) {
        if func.closure_env.is_empty() {
            return;
        }
        let start = reg_base + func.params.len();
        let limit = reg_base + n;
        for (i, value) in func.closure_env.iter().enumerate() {
            let slot = start + i;
            if slot < limit {
                self.regs[slot] = *value;
            }
        }
    }

    /// Reads a local variable from the current frame.
    pub fn local_get(&self, top: usize, slot: usize) -> Value {
        self.frames[top].locals[slot]
    }

    /// Writes a local variable in the current frame.
    pub fn local_set(&mut self, top: usize, slot: usize, value: Value) {
        self.frames[top].locals[slot] = value;
    }

    /// Returns a slice of local variables in the current frame.
    pub fn frame_locals(&self, top: usize) -> &[Value] {
        self.frames[top].locals.as_slice()
    }

    /// Returns a mutable slice of local variables in the current frame.
    #[allow(clippy::mut_from_ref)]
    pub fn frame_locals_mut(&mut self, top: usize) -> &mut [Value] {
        let frame = &self.frames[top];
        let len = frame.locals.len();
        unsafe { core::slice::from_raw_parts_mut(frame.locals.ptr, len) }
    }
}
