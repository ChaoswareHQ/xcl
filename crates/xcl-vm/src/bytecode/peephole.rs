//! Peephole optimizer: scans bytecode and fuses common instruction patterns.
//!
//! Runs after compilation to catch patterns that AST-level fusion misses.
//! Uses fixed-point iteration so cascading patterns (e.g., identity → NOP → eliminate) are caught.

use super::instr::Instr;
use super::opcodes::*;

/// Drain instructions in `range` and adjust all jump targets after `range.start()`.
#[inline]
fn drain_and_patch(
    code: &mut Vec<Instr>,
    wide: &mut [i64],
    range: core::ops::RangeInclusive<usize>,
) {
    let start = *range.start();
    let count = range.end() - start + 1;
    code.drain(range);
    let start_u32 = start as u32;
    let count_u32 = count as u32;
    for instr in code.iter_mut() {
        let op = instr.op();
        match op {
            OP_JUMP | OP_JUMP_IF_FALSE | OP_JUMP_IF_NIL | OP_FOR_ITER_NEXT | OP_FOR_RANGE_NEXT => {
                if instr.a() > start_u32 {
                    *instr = Instr::make(op, instr.a() - count_u32, instr.b(), instr.c());
                }
            }
            OP_WHILE_LT_INT | OP_WHILE_LT_LOCAL | OP_FOR_NUMBER_LOOP => {
                let w_idx = instr.c() as usize;
                if let Some(exit_ip) = wide.get_mut(w_idx)
                    && *exit_ip > start as i64
                {
                    *exit_ip -= count as i64;
                }
            }
            _ => {}
        }
    }
}

/// Peephole optimizer â€” scans bytecode and fuses common patterns.
/// This runs after compilation and catches patterns the AST-level fusion misses.
/// Uses fixed-point iteration: keeps scanning until no more optimizations are made,
/// so cascading patterns (e.g., identity â†’ NOP â†’ eliminate) are caught.
pub(crate) fn peephole_optimize_once(code: &mut Vec<Instr>, wide: &mut [i64]) {
    // Fixed-point iteration: repeat until no more changes
    loop {
        let mut changed = false;
        let mut i = 0;
        while i < code.len() {
            let matched = match code.get(i).copied() {
                Some(instr) => peephole_at(code, wide, i, instr),
                None => false,
            };
            if matched {
                changed = true;
                // Stay at same index to catch overlapping patterns
            } else {
                i += 1;
            }
        }
        // If no patterns matched in this pass, we're done
        if !changed {
            break;
        }
    }
}

/// Try to match and fuse a pattern at instruction index `i`.
/// Returns true if a fusion occurred (caller should not advance `i`).
#[inline]
fn peephole_at(code: &mut Vec<Instr>, wide: &mut [i64], i: usize, instr: Instr) -> bool {
    let op = instr.op();

    // Pattern 6: LOAD_LOCAL a; PUSH_INT 1; ADD; STORE_LOCAL a â†’ INC_LOCAL a
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_PUSH_INT
            && next.a() == 1
            && next2.op() == OP_ADD
            && next3.op() == OP_STORE_LOCAL
            && next3.a() == instr.a()
        {
            code[i] = Instr::a1(OP_INC_LOCAL, instr.a());
            drain_and_patch(code, wide, i + 1..=i + 3);
            return true;
        }
    }

    // Pattern 11: DEC_LOCAL â€” LOAD_LOCAL a; PUSH_INT 1; SUB; STORE_LOCAL a
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_PUSH_INT
            && next.a() == 1
            && next2.op() == OP_SUB
            && next3.op() == OP_STORE_LOCAL
            && next3.a() == instr.a()
        {
            code[i] = Instr::a1(OP_DEC_LOCAL, instr.a());
            drain_and_patch(code, wide, i + 1..=i + 3);
            return true;
        }
    }

    // Pattern 7: LOAD_LOCAL a; LOAD_LOCAL b; ADD; STORE_LOCAL a â†’ ADD_LOCAL_STORE a, b
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_LOAD_LOCAL && next3.op() == OP_STORE_LOCAL && next3.a() == instr.a() {
            let store_op = match next2.op() {
                OP_ADD => Some(OP_ADD_LOCAL_STORE),
                OP_SUB => Some(OP_SUB_LOCAL_STORE),
                OP_MUL => Some(OP_MUL_LOCAL_STORE),
                OP_DIV => Some(OP_DIV_LOCAL_STORE),
                _ => None,
            };
            if let Some(fused) = store_op {
                code[i] = Instr::a2(fused, instr.a(), next.a() as u16);
                drain_and_patch(code, wide, i + 1..=i + 3);
                return true;
            }
        }
    }

    // Pattern 9: LOAD_LOCAL a; LOAD_LOCAL b; ADD; STORE_LOCAL c â†’ ADD_LOCAL_LOCAL_STORE c, a, b
    // Like Luau's fused store pattern â€” eliminates 4 instructions into 1
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 && instr.a() < 256 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_LOAD_LOCAL && next3.op() == OP_STORE_LOCAL {
            let store_op = match next2.op() {
                OP_ADD => Some(OP_ADD_LOCAL_LOCAL_STORE),
                OP_SUB => Some(OP_SUB_LOCAL_LOCAL_STORE),
                OP_MUL => Some(OP_MUL_LOCAL_LOCAL_STORE),
                OP_DIV => Some(OP_DIV_LOCAL_LOCAL_STORE),
                OP_MOD => Some(OP_MOD_LOCAL_LOCAL_STORE),
                _ => None,
            };
            if let Some(fused) = store_op {
                // a=dest, b=src2, c=src1
                code[i] = Instr::make(fused, next3.a(), next.a() as u16, instr.a() as u8);
                drain_and_patch(code, wide, i + 1..=i + 3);
                return true;
            }
        }
    }

    // Pattern 2: LOAD_LOCAL a; PUSH_INT k; OP; STORE_LOCAL a â†’ ADD_INT_LOCAL a, k
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_PUSH_INT
            && u16::try_from(next.a()).is_ok()
            && next3.op() == OP_STORE_LOCAL
            && next3.a() == instr.a()
        {
            let arith_op = match next2.op() {
                OP_ADD => Some(OP_ADD_INT_LOCAL),
                OP_SUB => Some(OP_SUB_INT_LOCAL),
                OP_MUL => Some(OP_MUL_INT_LOCAL),
                _ => None,
            };
            if let Some(fused) = arith_op {
                code[i] = Instr::a2(fused, instr.a(), next.a() as u16);
                drain_and_patch(code, wide, i + 1..=i + 3);
                return true;
            }
        }
    }

    // Pattern 10: LOAD_LOCAL a; PUSH_INT k; ADD; STORE_LOCAL b â†’ ADD_INT_STORE_LOCAL b, a, k
    // Like Luau's ADDK â€” constant on the right side
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 && u16::try_from(instr.a()).is_ok() {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_PUSH_INT
            && u8::try_from(next.a()).is_ok()
            && next2.op() == OP_ADD
            && next3.op() == OP_STORE_LOCAL
        {
            code[i] = Instr::make(
                OP_ADD_INT_STORE_LOCAL,
                next3.a(),
                instr.a() as u16,
                next.a() as u8,
            );
            drain_and_patch(code, wide, i + 1..=i + 3);
            return true;
        }
    }

    // Pattern 1: LOAD_LOCAL a; LOAD_LOCAL b; ADD â†’ ADD_LOCAL_LOCAL a, b
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_LOAD_LOCAL && next3.op() == OP_STORE_LOCAL {
            let arith_op = match next2.op() {
                OP_ADD => Some(OP_ADD_LOCAL_LOCAL),
                OP_SUB => Some(OP_SUB_LOCAL_LOCAL),
                OP_MUL => Some(OP_MUL_LOCAL_LOCAL),
                OP_DIV => Some(OP_DIV_LOCAL_LOCAL),
                OP_LT => Some(OP_LT_LOCAL_LOCAL),
                _ => None,
            };
            if let Some(fused) = arith_op {
                code[i] = Instr::make(fused, instr.a(), next.a() as u16, 0);
                drain_and_patch(code, wide, i + 1..=i + 2);
                return true;
            }
        }
    }

    // Pattern 3: PUSH_INT a; PUSH_INT b; ADD â†’ PUSH_INT (a+b) (constant folding)
    if op == OP_PUSH_INT && code.len() >= i + 3 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        if next.op() == OP_PUSH_INT {
            let a = instr.a() as i64;
            let b = next.a() as i64;
            let result = match next2.op() {
                OP_ADD => Some(a + b),
                OP_SUB => Some(a - b),
                OP_MUL => Some(a * b),
                OP_DIV if b != 0 => Some(a / b),
                _ => None,
            };
            if let Some(r) = result {
                if (0..=u32::MAX as i64).contains(&r) {
                    code[i] = Instr::a1(OP_PUSH_INT, r as u32);
                    drain_and_patch(code, wide, i + 1..=i + 2);
                    return true;
                }
            }
        }
    }

    // Pattern 5: LOAD_LOCAL a; STORE_LOCAL a â†’ (delete both, no-op)
    if op == OP_LOAD_LOCAL && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_STORE_LOCAL && next.a() == instr.a() {
            drain_and_patch(code, wide, i..=i + 1);
            return true;
        }
    }

    // Pattern 8: LOAD_LOCAL a; LOAD_LOCAL b; LOAD_LOCAL c; ADD; ADD â†’ fused chain
    if op == OP_LOAD_LOCAL && code.len() >= i + 5 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        let next4 = code[i + 4];
        if next.op() == OP_LOAD_LOCAL
            && next2.op() == OP_LOAD_LOCAL
            && next3.op() == OP_ADD
            && next4.op() == OP_ADD
        {
            code[i] = Instr::make(OP_ADD_LOCAL_LOCAL, instr.a(), next.a() as u16, 0);
            code[i + 1] = next2;
            code[i + 2] = next3;
            drain_and_patch(code, wide, i + 3..=i + 4);
            return true;
        }
    }

    // Pattern 12: Dead code elimination â€” consecutive NOPs
    if op == OP_NOP && code.len() >= i + 2 && code[i + 1].op() == OP_NOP {
        drain_and_patch(code, wide, i..=i + 1);
        return true;
    }

    // Pattern 13: PUSH_INT a; STORE_LOCAL b â†’ LOAD_CONST_LOCAL
    // Fuses constant integer assignment into a single instruction.
    // Encoding: a=slot, b=value (16-bit), c=0 for inline int.
    // Only fuses when value fits in 16 bits (most common case).
    if op == OP_PUSH_INT && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_STORE_LOCAL {
            let int_val = instr.a();
            let slot = next.a();
            if u16::try_from(int_val).is_ok() {
                code[i] = Instr::make(OP_LOAD_CONST_LOCAL, slot, int_val as u16, 0);
                drain_and_patch(code, wide, i + 1..=i + 1);
                return true;
            }
        }
    }

    // Pattern 14: PUSH_FLOAT a; STORE_LOCAL b â†’ LOAD_CONST_LOCAL b, wide_idx, 1
    if op == OP_PUSH_FLOAT && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_STORE_LOCAL {
            let w = instr.a() as u16;
            let slot = next.a();
            code[i] = Instr::make(OP_LOAD_CONST_LOCAL, slot, w, 1);
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // Pattern 15: PUSH_STR a; STORE_LOCAL b â†’ LOAD_CONST_LOCAL with kind=2
    if op == OP_PUSH_STR && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_STORE_LOCAL {
            let str_idx = instr.a() as u16;
            let slot = next.a();
            code[i] = Instr::make(OP_LOAD_CONST_LOCAL, slot, str_idx, 2);
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // Pattern 16: PUSH_NIL/TRUE/FALSE; STORE_LOCAL b â†’ fused boolean/nil store
    // Encode as LOAD_CONST_LOCAL with c=3 for nil, c=4 for true, c=5 for false
    if (op == OP_PUSH_NIL || op == OP_PUSH_TRUE || op == OP_PUSH_FALSE) && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_STORE_LOCAL {
            let kind = match op {
                OP_PUSH_NIL => 3,
                OP_PUSH_TRUE => 4,
                OP_PUSH_FALSE => 5,
                _ => 0,
            };
            let slot = next.a();
            code[i] = Instr::make(OP_LOAD_CONST_LOCAL, slot, 0, kind);
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // Pattern 17: PUSH_INT_WIDE a; STORE_LOCAL b â†’ LOAD_CONST_LOCAL b, a, 0
    if op == OP_PUSH_INT_WIDE && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_STORE_LOCAL {
            let w = instr.a() as u16;
            let slot = next.a();
            code[i] = Instr::make(OP_LOAD_CONST_LOCAL, slot, w, 0);
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // === NEW ALGEBRAIC PATTERNS ===

    // Pattern 18: Identity addition â€” PUSH_INT 0; ADD â†’ NOP (x + 0 = x)
    // The ADD pops two values and pushes the sum; with 0 on top, it just returns the bottom value.
    // Replace ADD with NOP so the bottom value stays on the stack, then remove the PUSH_INT 0.
    if op == OP_PUSH_INT && instr.a() == 0 && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_ADD {
            // Remove PUSH_INT 0, replace ADD with NOP (value already on stack)
            code[i] = Instr::none(OP_NOP);
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // Pattern 19: Identity multiplication â€” PUSH_INT 1; MUL â†’ NOP (x * 1 = x)
    if op == OP_PUSH_INT && instr.a() == 1 && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_MUL {
            code[i] = Instr::none(OP_NOP);
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // Pattern 20: Identity division â€” PUSH_INT 1; DIV â†’ NOP (x / 1 = x)
    if op == OP_PUSH_INT && instr.a() == 1 && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_DIV {
            code[i] = Instr::none(OP_NOP);
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // Pattern 21: Zero multiplication â€” PUSH_INT 0; MUL â†’ PUSH_INT 0 (x * 0 = 0)
    // Replace the PUSH_INT 0 at position i with NOP (remove the x), replace MUL with PUSH_INT 0
    if op == OP_PUSH_INT && instr.a() == 0 && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_MUL {
            code[i] = Instr::a1(OP_PUSH_INT, 0);
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // Pattern 22: Self subtraction â€” LOAD_LOCAL a; LOAD_LOCAL a; SUB â†’ PUSH_INT 0 (x - x = 0)
    if op == OP_LOAD_LOCAL && code.len() >= i + 3 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        if next.op() == OP_LOAD_LOCAL
            && next.a() == instr.a()
            && next2.op() == OP_SUB
        {
            code[i] = Instr::a1(OP_PUSH_INT, 0);
            drain_and_patch(code, wide, i + 1..=i + 2);
            return true;
        }
    }

    // Pattern 23: Redundant load elimination â€” LOAD_LOCAL a; LOAD_LOCAL a â†’ LOAD_LOCAL a
    if op == OP_LOAD_LOCAL && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_LOAD_LOCAL && next.a() == instr.a() {
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // Pattern 24: Redundant jump elimination â€” JUMP a; JUMP b â†’ JUMP b
    // Two consecutive jumps: the first is unreachable after the second, keep only the second
    if op == OP_JUMP && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_JUMP {
            code[i] = next;
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // Pattern 25: JUMP_IF_FALSE a; JUMP b where target=a â†’ JUMP b
    // If JUMP_IF_FALSE falls through to an unconditional JUMP, and the false target is the same
    // as the unconditional jump target, just use the unconditional jump.
    if op == OP_JUMP_IF_FALSE && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_JUMP && next.a() == instr.a() {
            // Both branches go to the same place â€” just jump unconditionally
            code[i] = Instr::a1(OP_JUMP, instr.a());
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // Pattern 26: Strength reduction â€” LOAD_LOCAL a; PUSH_INT (2^n); MUL; STORE_LOCAL a
    // â†’ MUL_INT_LOCAL a, (2^n) when the result fits, already handled by Pattern 2.
    // Instead: fold PUSH_INT a; PUSH_INT b; MUL when both are constants (covered by Pattern 3).
    // Add: PUSH_INT 0; SUB â†’ PUSH_INT 0 (0 - x doesn't simplify, but x - 0 = x via Pattern 18).
    // New: PUSH_INT a; PUSH_INT b; MOD â†’ PUSH_INT (a % b) for constant modulo folding.
    if op == OP_PUSH_INT && code.len() >= i + 3 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        if next.op() == OP_PUSH_INT {
            let a = instr.a() as i64;
            let b = next.a() as i64;
            if next2.op() == OP_MOD && b != 0 {
                let result = a % b;
                if (0..=u32::MAX as i64).contains(&result) {
                    code[i] = Instr::a1(OP_PUSH_INT, result as u32);
                    drain_and_patch(code, wide, i + 1..=i + 2);
                    return true;
                }
            }
        }
    }

    // Pattern 27: Constant subtraction folding â€” PUSH_INT a; PUSH_INT b; SUB â†’ PUSH_INT (a-b)
    // Already covered by Pattern 3, but adding NEG folding: PUSH_INT a; NEG â†’ PUSH_INT (-a)
    if op == OP_PUSH_INT && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_NEG {
            let val = instr.a() as i64;
            // Negate the constant; handle overflow by keeping as runtime NEG
            if let Some(negated) = val.checked_neg() {
                if (0..=u32::MAX as i64).contains(&negated) {
                    code[i] = Instr::a1(OP_PUSH_INT, negated as u32);
                    drain_and_patch(code, wide, i + 1..=i + 1);
                    return true;
                }
            }
        }
    }

    // Pattern 28: Single NOP elimination (non-consecutive) â€” isolated NOP â†’ removed
    // This catches NOPs left by Patterns 18-20 after the consecutive NOP pattern runs
    if op == OP_NOP {
        drain_and_patch(code, wide, i..=i);
        return true;
    }

    // === NEW SUPEROPERATORS ===

    // Pattern 29: Float constant fusion — LOAD_LOCAL a; PUSH_FLOAT w; OP; STORE_LOCAL a
    // → OP_ADD_FLOAT_LOCAL a, w (like ADD_INT_LOCAL but for floats)
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_PUSH_FLOAT
            && next3.op() == OP_STORE_LOCAL
            && next3.a() == instr.a()
        {
            let arith_op = match next2.op() {
                OP_ADD => Some(OP_ADD_FLOAT_LOCAL),
                OP_SUB => Some(OP_SUB_FLOAT_LOCAL),
                OP_MUL => Some(OP_MUL_FLOAT_LOCAL),
                OP_DIV => Some(OP_DIV_FLOAT_LOCAL),
                _ => None,
            };
            if let Some(fused) = arith_op {
                code[i] = Instr::a2(fused, instr.a(), next.a() as u16);
                drain_and_patch(code, wide, i + 1..=i + 3);
                return true;
            }
        }
    }

    // Pattern 30: Modulo+equality fusion — LOAD_LOCAL a; PUSH_INT d; MOD; PUSH_INT v; EQ
    // → OP_MOD_EQ_INT a, d, v (for $i % 2 == 0 patterns)
    if op == OP_LOAD_LOCAL && code.len() >= i + 5 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        let next4 = code[i + 4];
        if next.op() == OP_PUSH_INT
            && u16::try_from(next.a()).is_ok()
            && next2.op() == OP_MOD
            && next3.op() == OP_PUSH_INT
            && u8::try_from(next3.a()).is_ok()
            && next4.op() == OP_EQ
        {
            code[i] = Instr::make(
                OP_MOD_EQ_INT,
                instr.a(),
                next.a() as u16,
                next3.a() as u8,
            );
            drain_and_patch(code, wide, i + 1..=i + 4);
            return true;
        }
    }

    // === STRENGTH REDUCTION ===

    // Pattern 31: Multiply by power of 2 → shift — PUSH_INT (2^n); MUL → PUSH_INT n; SHL
    // e.g. $x * 4 → $x << 2
    if op == OP_PUSH_INT && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_MUL {
            let val = instr.a() as u64;
            if val > 0 && val.is_power_of_two() && val <= 64 {
                let shift = val.trailing_zeros();
                code[i] = Instr::a1(OP_PUSH_INT, shift);
                code[i + 1] = Instr::none(OP_SHL);
                return true;
            }
        }
    }

    // Pattern 32: Divide by power of 2 → shift — PUSH_INT (2^n); DIV → PUSH_INT n; SHR
    // e.g. $x / 8 → $x >> 3 (only for positive integers)
    if op == OP_PUSH_INT && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_DIV {
            let val = instr.a() as u64;
            if val > 0 && val.is_power_of_two() && val <= 64 {
                let shift = val.trailing_zeros();
                code[i] = Instr::a1(OP_PUSH_INT, shift);
                code[i + 1] = Instr::none(OP_SHR);
                return true;
            }
        }
    }

    // Pattern 33: Modulo by power of 2 → bitwise AND — PUSH_INT (2^n); MOD → PUSH_INT (2^n-1); BITAND
    // e.g. $x % 8 → $x & 7
    if op == OP_PUSH_INT && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_MOD {
            let val = instr.a() as u64;
            if val > 0 && val.is_power_of_two() {
                let mask = val - 1;
                if u32::try_from(mask).is_ok() {
                    code[i] = Instr::a1(OP_PUSH_INT, mask as u32);
                    code[i + 1] = Instr::none(OP_BITAND);
                    return true;
                }
            }
        }
    }

    // Pattern 34: Constant shift folding — PUSH_INT a; PUSH_INT b; SHL → PUSH_INT (a << b)
    if op == OP_PUSH_INT && code.len() >= i + 3 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        if next.op() == OP_PUSH_INT && (next2.op() == OP_SHL || next2.op() == OP_SHR) {
            let a = instr.a() as i64;
            let b = next.a();
            if b < 64 {
                let result = if next2.op() == OP_SHL {
                    (a as i128) << b
                } else {
                    (a as i128) >> b
                };
                if result >= 0 && result <= u32::MAX as i128 {
                    code[i] = Instr::a1(OP_PUSH_INT, result as u32);
                    drain_and_patch(code, wide, i + 1..=i + 2);
                    return true;
                }
            }
        }
    }

    // Pattern 35: Constant bitwise AND folding — PUSH_INT a; PUSH_INT b; BITAND → PUSH_INT (a & b)
    if op == OP_PUSH_INT && code.len() >= i + 3 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        if next.op() == OP_PUSH_INT && next2.op() == OP_BITAND {
            let a = instr.a() as i64;
            let b = next.a() as i64;
            let result = a & b;
            if u32::try_from(result).is_ok() {
                code[i] = Instr::a1(OP_PUSH_INT, result as u32);
                drain_and_patch(code, wide, i + 1..=i + 2);
                return true;
            }
        }
    }

    false
}
