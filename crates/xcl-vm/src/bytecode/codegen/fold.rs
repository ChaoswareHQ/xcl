//! Compile-time folding of counted loops.
//!
//! A loop whose trip count and accumulator are known before it starts does not
//! have to run:
//!
//! ```xcl
//! $i = 0
//! $s = 0
//! while $i < 10000 {
//!     $s = $s + $i
//!     $i = $i + 1
//! }
//! ```
//!
//! is `$s = 49995000` and `$i = 10000`. This is the transformation LLVM's
//! `loop-idiom` and scalar-evolution passes perform, restricted to the shape an
//! interpreter can prove cheaply: a counter that starts at a known integer,
//! steps by one, ends at a known bound, and a body that only adds an affine
//! function of that counter into known-integer accumulators.
//!
//! # Soundness
//!
//! Folding turns a loop into two stores, so the conditions are strict:
//!
//! * the counter, the bound, and every accumulator have compile-time integer
//!   values (tracked by `Compiler::const_locals`);
//! * every body statement is `$acc = $acc + <term>` with `<term>` either the
//!   counter or a non-negative integer literal, plus (for `while`) one final
//!   `$i = $i + 1`;
//! * every term is non-negative, so each partial sum lies between zero and the
//!   total. A single range check on the total then covers every intermediate
//!   value, and the loop would have stayed in integer arithmetic too;
//! * the total and each accumulator's final value fit in the 48-bit range.
//!
//! Anything else falls back to the loop. A wrong fold here would silently change
//! a program's answers, so the pass prefers to miss opportunities.

use super::*;
use crate::runtime::vm::ops::consts::{OP_LOAD_CONST_LOCAL, OP_PUSH_INT_WIDE, OP_STORE_LOCAL};

/// Largest integer the runtime stores inline (`i48`).
const MAX_I48: i128 = (1 << 47) - 1;

/// One `$acc = $acc + ...` seen in a loop body.
struct Accumulator {
    name: String,
    /// How many times the counter is added per iteration.
    counter_coeff: i128,
    /// Constant added per iteration.
    constant: i128,
    /// The accumulator's value before the loop.
    start: i128,
}

/// What a body statement adds to the accumulator.
enum Term<'a> {
    /// A bare identifier; it must be the loop counter.
    Ident(&'a str),
    /// An integer literal.
    Constant(i64),
}

impl Compiler<'_> {
    /// Folds `while $i < end { $acc = $acc + e; $i = $i + 1 }`, if provable.
    pub(super) fn fold_while_sum(
        &mut self,
        entry: &HashMap<Box<str>, i64>,
        var: &str,
        end: i64,
        body: &[AstNode],
    ) -> bool {
        let Some(&start) = entry.get(var) else {
            return false;
        };
        self.fold_counted_sum(entry, var, start, end, true, body)
    }

    /// Folds `for $i in start..end { $acc = $acc + e }`.
    ///
    /// The header assigns the counter, so its entry value is `start` and its
    /// value after a normal exit is `end`.
    pub(super) fn fold_for_sum(
        &mut self,
        entry: &HashMap<Box<str>, i64>,
        var: &str,
        start: i64,
        end: i64,
        body: &[AstNode],
    ) -> bool {
        self.fold_counted_sum(entry, var, start, end, false, body)
    }

    fn fold_counted_sum(
        &mut self,
        entry: &HashMap<Box<str>, i64>,
        var: &str,
        start: i64,
        end: i64,
        steps: bool,
        body: &[AstNode],
    ) -> bool {
        let trips = i128::from(end) - i128::from(start);
        // Zero-trip loops still assign the counter in the `for` header, so leave
        // them to the normal path; negative ranges are not counted loops.
        if trips <= 0 || start < 0 || end < 0 {
            return false;
        }

        let (accumulator_stmts, step_stmt) = if steps {
            match body.split_last() {
                Some((last, rest)) => (rest, Some(last)),
                None => return false,
            }
        } else {
            (body, None)
        };
        if accumulator_stmts.is_empty() {
            return false;
        }

        if let Some(step) = step_stmt {
            match step {
                AstNode::Assign { name, value, .. } if name == var && is_plus_one(value, var) => {}
                _ => return false,
            }
        }

        let mut accumulators: Vec<Accumulator> = Vec::new();
        for stmt in accumulator_stmts {
            let AstNode::Assign { name, value, .. } = stmt else {
                return false;
            };
            if name == var {
                return false;
            }
            let Some(term) = adds_to_self(value, name) else {
                return false;
            };
            let Some(&start_value) = entry.get(name.as_str()) else {
                return false;
            };
            let (counter_coeff, constant) = match term {
                Term::Ident(ident) if ident == var => (1i128, 0i128),
                Term::Constant(c) if c >= 0 => (0i128, i128::from(c)),
                _ => return false,
            };
            match accumulators.iter_mut().find(|a| a.name == *name) {
                Some(existing) => {
                    existing.counter_coeff += counter_coeff;
                    existing.constant += constant;
                }
                None => accumulators.push(Accumulator {
                    name: name.clone(),
                    counter_coeff,
                    constant,
                    start: i128::from(start_value),
                }),
            }
        }

        // Closed forms: the sum of the counter over the trip, and the trips.
        let counter_sum = (i128::from(start) + i128::from(end) - 1) * trips / 2;
        let mut folded: Vec<(String, i64)> = Vec::with_capacity(accumulators.len());
        for acc in &accumulators {
            let total = acc.counter_coeff * counter_sum + acc.constant * trips;
            let final_value = acc.start + total;
            if total < 0 || !(0..=MAX_I48).contains(&final_value) {
                return false;
            }
            folded.push((acc.name.clone(), final_value as i64));
        }

        for (name, value) in folded {
            let slot = self.local(&name);
            self.store_int(slot, value);
            self.const_locals.insert(name.into_boxed_str(), value);
        }
        let counter_slot = self.local(var);
        self.store_int(counter_slot, end);
        self.const_locals.insert(var.into(), end);
        true
    }

    /// Stores an integer literal into `slot`.
    fn store_int(&mut self, slot: u32, value: i64) {
        if let Ok(v) = u16::try_from(value) {
            self.emit(Instr::make(OP_LOAD_CONST_LOCAL, slot, v, 0));
        } else if let Ok(w) = u16::try_from(self.wide(value)) {
            self.emit(Instr::make(OP_LOAD_CONST_LOCAL, slot, w, 6));
        } else {
            let w = self.wide(value);
            self.emit(Instr::a1(OP_PUSH_INT_WIDE, w));
            self.emit(Instr::a1(OP_STORE_LOCAL, slot));
        }
    }
}

/// Does `value` read as `$var + 1`?
fn is_plus_one(value: &AstNode, var: &str) -> bool {
    matches!(
        value,
        AstNode::BinaryOp { left, op: BinaryOperator::Add, right, .. }
            if matches!(left.as_ref(), AstNode::Ident { name, .. } if name == var)
                && matches!(right.as_ref(), AstNode::Int { value: 1, .. })
    )
}

/// `$acc = $acc + term` → `Some(term)`.
fn adds_to_self<'a>(value: &'a AstNode, name: &str) -> Option<Term<'a>> {
    let AstNode::BinaryOp {
        left,
        op: BinaryOperator::Add,
        right,
        ..
    } = value
    else {
        return None;
    };
    match left.as_ref() {
        AstNode::Ident {
            name: left_name, ..
        } if left_name == name => {}
        _ => return None,
    }
    match right.as_ref() {
        AstNode::Ident { name, .. } => Some(Term::Ident(name)),
        AstNode::Int { value, .. } => Some(Term::Constant(*value)),
        _ => None,
    }
}
