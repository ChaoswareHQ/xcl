//! Peephole optimizer: scans bytecode and fuses common instruction patterns.
//!
//! Runs after compilation to catch patterns that AST-level fusion misses.
//! Uses fixed-point iteration so cascading patterns (e.g., identity ? NOP ? eliminate) are caught.


// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use alloc::{
    vec::Vec,
};

use super::instr::Instr;
use super::opcodes::*;

#[inline(always)]
fn is_direct_jump(op: u8) -> bool {
    matches!(
        op,
        OP_JUMP | OP_JUMP_IF_FALSE | OP_JUMP_IF_NIL | OP_FOR_ITER_NEXT | OP_FOR_RANGE_NEXT
    )
}

#[inline(always)]
fn is_wide_exit_jump(op: u8) -> bool {
    matches!(
        op,
        OP_WHILE_LT_INT | OP_WHILE_LT_LOCAL | OP_FOR_NUMBER_LOOP | OP_IF_LT_INT | OP_IF_EQ_INT
    )
}

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
        if is_direct_jump(op) {
            if instr.a() > start_u32 {
                *instr = Instr::make(op, instr.a() - count_u32, instr.b(), instr.c());
            }
        } else if is_wide_exit_jump(op) {
            let w_idx = instr.c() as usize;
            if let Some(exit_ip) = wide.get_mut(w_idx)
                && *exit_ip > start as i64
            {
                *exit_ip -= count as i64;
            }
        }
    }
}

/// Collect all instruction indices that are jump targets.
fn collect_jump_targets(code: &[Instr], wide: &[i64]) -> crate::HashSet<usize> {
    let mut targets = crate::HashSet::new();
    for instr in code {
        let op = instr.op();
        if is_direct_jump(op) {
            let target = instr.a() as usize;
            if target < code.len() {
                targets.insert(target);
            }
        } else if is_wide_exit_jump(op) {
            let target = wide.get(instr.c() as usize).copied().unwrap_or(0) as usize;
            if target < code.len() {
                targets.insert(target);
            }
        }
    }
    targets
}

/// LOAD_LOCAL a; PUSH_INT 1; ADD/SUB; STORE_LOCAL a -> INC_LOCAL/DEC_LOCAL a
fn try_fuse_inc_dec(
    code: &mut Vec<Instr>,
    wide: &mut [i64],
    i: usize,
    instr: Instr,
) -> bool {
    if instr.op() != OP_LOAD_LOCAL || code.len() < i + 4 {
        return false;
    }
    let next = code[i + 1];
    let next2 = code[i + 2];
    let next3 = code[i + 3];
    if next.op() != OP_PUSH_INT
        || next.a() != 1
        || next3.op() != OP_STORE_LOCAL
        || next3.a() != instr.a()
    {
        return false;
    }
    let fused = match next2.op() {
        OP_ADD => OP_INC_LOCAL,
        OP_SUB => OP_DEC_LOCAL,
        _ => return false,
    };
    code[i] = Instr::a1(fused, instr.a());
    drain_and_patch(code, wide, i + 1..=i + 3);
    true
}

/// PUSH_<const>; STORE_LOCAL -> LOAD_CONST_LOCAL
///
/// Covers PUSH_INT, PUSH_FLOAT, PUSH_STR, PUSH_NIL/TRUE/FALSE, and PUSH_INT_WIDE.
/// All of these must respect the same jump-target guards.
fn try_fuse_const_store(
    code: &mut Vec<Instr>,
    wide: &mut [i64],
    i: usize,
    instr: Instr,
    jump_targets: &crate::HashSet<usize>,
) -> bool {
    let op = instr.op();
    if !matches!(
        op,
        OP_PUSH_INT
            | OP_PUSH_FLOAT
            | OP_PUSH_STR
            | OP_PUSH_NIL
            | OP_PUSH_TRUE
            | OP_PUSH_FALSE
            | OP_PUSH_INT_WIDE
    ) {
        return false;
    }
    if code.len() < i + 2
        || jump_targets.contains(&i)
        || jump_targets.contains(&(i + 1))
    {
        return false;
    }
    if i > 0 && code[i - 1].op() == OP_JUMP {
        return false;
    }
    let next = code[i + 1];
    if next.op() != OP_STORE_LOCAL {
        return false;
    }
    let slot = next.a();
    let (b, c) = match op {
        OP_PUSH_INT => {
            let v = instr.a();
            if u16::try_from(v).is_err() {
                return false;
            }
            (v as u16, 0)
        }
        OP_PUSH_FLOAT | OP_PUSH_STR | OP_PUSH_INT_WIDE => (instr.a() as u16, match op {
            OP_PUSH_FLOAT => 1,
            OP_PUSH_STR => 2,
            OP_PUSH_INT_WIDE => 6,
            _ => 0,
        }),
        OP_PUSH_NIL => (0, 3),
        OP_PUSH_TRUE => (0, 4),
        OP_PUSH_FALSE => (0, 5),
        _ => unreachable!(),
    };
    code[i] = Instr::make(OP_LOAD_CONST_LOCAL, slot, b, c);
    drain_and_patch(code, wide, i + 1..=i + 1);
    true
}

/// PUSH_INT k combined with a following ADD/MUL/DIV/MOD.
///
/// Preserves the existing rule order:
/// 1. k + 0 -> NOP
/// 2. k * 1 -> NOP
/// 3. k / 1 -> NOP
/// 4. k * 0 -> PUSH_INT 0
/// 5. 2^n * -> shift
/// 6. 2^n / -> shift
/// 7. 2^n % -> bitwise AND
fn try_fuse_push_int_op(
    code: &mut Vec<Instr>,
    wide: &mut [i64],
    i: usize,
    instr: Instr,
) -> bool {
    if instr.op() != OP_PUSH_INT || code.len() < i + 2 {
        return false;
    }
    let next = code[i + 1];
    let val = instr.a();
    let uval = val as u64;

    if next.op() == OP_ADD && val == 0 {
        code[i] = Instr::none(OP_NOP);
        drain_and_patch(code, wide, i + 1..=i + 1);
        return true;
    }
    if next.op() == OP_MUL && val == 1 {
        code[i] = Instr::none(OP_NOP);
        drain_and_patch(code, wide, i + 1..=i + 1);
        return true;
    }
    if next.op() == OP_DIV && val == 1 {
        code[i] = Instr::none(OP_NOP);
        drain_and_patch(code, wide, i + 1..=i + 1);
        return true;
    }
    if next.op() == OP_MUL && val == 0 {
        code[i] = Instr::a1(OP_PUSH_INT, 0);
        drain_and_patch(code, wide, i + 1..=i + 1);
        return true;
    }

    if uval == 0 || !uval.is_power_of_two() {
        return false;
    }

    match next.op() {
        OP_MUL if uval <= 64 => {
            code[i] = Instr::a1(OP_PUSH_INT, uval.trailing_zeros());
            code[i + 1] = Instr::none(OP_SHL);
            true
        }
        OP_DIV if uval <= 64 => {
            code[i] = Instr::a1(OP_PUSH_INT, uval.trailing_zeros());
            code[i + 1] = Instr::none(OP_SHR);
            true
        }
        OP_MOD => {
            let mask = uval - 1;
            if u32::try_from(mask).is_ok() {
                code[i] = Instr::a1(OP_PUSH_INT, mask as u32);
                code[i + 1] = Instr::none(OP_BITAND);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}
/// Peephole optimizer — scans bytecode and fuses common patterns.
/// This runs after compilation and catches patterns the AST-level fusion misses.
/// Uses fixed-point iteration: keeps scanning until no more optimizations are made,
/// so cascading patterns (e.g., identity → NOP → eliminate) are caught.
pub(crate) fn peephole_optimize_once(code: &mut Vec<Instr>, wide: &mut [i64]) {
    // Fixed-point iteration: repeat until no more changes
    loop {
        let jump_targets = collect_jump_targets(code, wide);
        let mut changed = false;
        let mut i = 0;
        while i < code.len() {
            let matched = match code.get(i).copied() {
                Some(instr) => peephole_at(code, wide, i, instr, &jump_targets),
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
fn peephole_at(
    code: &mut Vec<Instr>,
    wide: &mut [i64],
    i: usize,
    instr: Instr,
    jump_targets: &crate::HashSet<usize>,
) -> bool {
    let op = instr.op();

    if try_fuse_inc_dec(code, wide, i, instr) {
        return true;
    }

    // Pattern 7: LOAD_LOCAL a; LOAD_LOCAL b; ADD; STORE_LOCAL a → ADD_LOCAL_STORE a, b
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

    // Pattern 9: LOAD_LOCAL a; LOAD_LOCAL b; ADD; STORE_LOCAL c → ADD_LOCAL_LOCAL_STORE c, a, b
    // Like Luau's fused store pattern — eliminates 4 instructions into 1
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

    // Pattern 2: LOAD_LOCAL a; PUSH_INT k; OP; STORE_LOCAL a → ADD_INT_LOCAL a, k
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

    // Pattern 10: LOAD_LOCAL a; PUSH_INT k; ADD; STORE_LOCAL b → ADD_INT_STORE_LOCAL b, a, k
    // Like Luau's ADDK — constant on the right side
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

    // Pattern 1: LOAD_LOCAL a; LOAD_LOCAL b; ADD → ADD_LOCAL_LOCAL a, b
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

    // Pattern 3: PUSH_INT a; PUSH_INT b; ADD → PUSH_INT (a+b) (constant folding)
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

    // Pattern 5: LOAD_LOCAL a; STORE_LOCAL a → (delete both, no-op)
    if op == OP_LOAD_LOCAL && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_STORE_LOCAL && next.a() == instr.a() {
            drain_and_patch(code, wide, i..=i + 1);
            return true;
        }
    }

    // Pattern 8: LOAD_LOCAL a; LOAD_LOCAL b; LOAD_LOCAL c; ADD; ADD → fused chain
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

    // Pattern 12: Dead code elimination — consecutive NOPs
    if op == OP_NOP && code.len() >= i + 2 && code[i + 1].op() == OP_NOP {
        drain_and_patch(code, wide, i..=i + 1);
        return true;
    }

    if try_fuse_const_store(code, wide, i, instr, &jump_targets) {
        return true;
    }

    if try_fuse_push_int_op(code, wide, i, instr) {
        return true;
    }

    // Pattern 22: Self subtraction — LOAD_LOCAL a; LOAD_LOCAL a; SUB → PUSH_INT 0 (x - x = 0)
    if op == OP_LOAD_LOCAL && code.len() >= i + 3 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        if next.op() == OP_LOAD_LOCAL && next.a() == instr.a() && next2.op() == OP_SUB {
            code[i] = Instr::a1(OP_PUSH_INT, 0);
            drain_and_patch(code, wide, i + 1..=i + 2);
            return true;
        }
    }

    // Pattern 23: Redundant load elimination — LOAD_LOCAL a; LOAD_LOCAL a → LOAD_LOCAL a
    if op == OP_LOAD_LOCAL && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_LOAD_LOCAL && next.a() == instr.a() {
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // Pattern 24: Redundant jump elimination — JUMP a; JUMP b → JUMP b
    // Two consecutive jumps: the first is unreachable after the second, keep only the second
    if op == OP_JUMP && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_JUMP {
            code[i] = next;
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // Pattern 25: JUMP_IF_FALSE a; JUMP b where target=a → JUMP b
    // If JUMP_IF_FALSE falls through to an unconditional JUMP, and the false target is the same
    // as the unconditional jump target, just use the unconditional jump.
    if op == OP_JUMP_IF_FALSE && code.len() >= i + 2 {
        let next = code[i + 1];
        if next.op() == OP_JUMP && next.a() == instr.a() {
            // Both branches go to the same place — just jump unconditionally
            code[i] = Instr::a1(OP_JUMP, instr.a());
            drain_and_patch(code, wide, i + 1..=i + 1);
            return true;
        }
    }

    // Pattern 26: Strength reduction — LOAD_LOCAL a; PUSH_INT (2^n); MUL; STORE_LOCAL a
    // → MUL_INT_LOCAL a, (2^n) when the result fits, already handled by Pattern 2.
    // Instead: fold PUSH_INT a; PUSH_INT b; MUL when both are constants (covered by Pattern 3).
    // Add: PUSH_INT 0; SUB → PUSH_INT 0 (0 - x doesn't simplify, but x - 0 = x via Pattern 18).
    // New: PUSH_INT a; PUSH_INT b; MOD → PUSH_INT (a % b) for constant modulo folding.
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

    // Pattern 27: Constant subtraction folding — PUSH_INT a; PUSH_INT b; SUB → PUSH_INT (a-b)
    // Already covered by Pattern 3, but adding NEG folding: PUSH_INT a; NEG → PUSH_INT (-a)
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

    // Pattern 28: Single NOP elimination (non-consecutive) — isolated NOP → removed
    // This catches NOPs left by Patterns 18-20 after the consecutive NOP pattern runs
    if op == OP_NOP {
        drain_and_patch(code, wide, i..=i);
        return true;
    }

    // === NEW SUPEROPERATORS ===

    // Pattern 29: Float constant fusion � LOAD_LOCAL a; PUSH_FLOAT w; OP; STORE_LOCAL a
    // ? OP_ADD_FLOAT_LOCAL a, w (like ADD_INT_LOCAL but for floats)
    if op == OP_LOAD_LOCAL && code.len() >= i + 4 {
        let next = code[i + 1];
        let next2 = code[i + 2];
        let next3 = code[i + 3];
        if next.op() == OP_PUSH_FLOAT && next3.op() == OP_STORE_LOCAL && next3.a() == instr.a() {
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

    // Pattern 30: Modulo+equality fusion � LOAD_LOCAL a; PUSH_INT d; MOD; PUSH_INT v; EQ
    // ? OP_MOD_EQ_INT a, d, v (for $i % 2 == 0 patterns)
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
            code[i] = Instr::make(OP_MOD_EQ_INT, instr.a(), next.a() as u16, next3.a() as u8);
            drain_and_patch(code, wide, i + 1..=i + 4);
            return true;
        }
    }

    if try_fuse_push_int_op(code, wide, i, instr) {
        return true;
    }

    // Pattern 34: Constant shift folding � PUSH_INT a; PUSH_INT b; SHL ? PUSH_INT (a << b)
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

    // Pattern 35: Constant bitwise AND folding � PUSH_INT a; PUSH_INT b; BITAND ? PUSH_INT (a & b)
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
