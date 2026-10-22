//! Statement and expression compilation.
//!
//! [`Compiler::node`] is the AST dispatcher: one `match` arm per node kind,
//! emitting the instructions for that construct. It is the largest part of the
//! compiler and the only place where source syntax is translated.

use super::inline::{is_simple_arg, scan_vars, substitute};
use super::opcode_map::*;
use super::*;

impl Compiler<'_> {
    pub(super) fn node(&mut self, node: &AstNode) {
        // Constant tracking for the counted-loop fold. A literal assignment is
        // recorded; anything that can change a local without naming it as a
        // literal invalidates the whole set.
        match node {
            AstNode::Assign { name, value, .. } => match value.as_ref() {
                AstNode::Int { value, .. } => {
                    self.const_locals.insert(name.as_str().into(), *value);
                }
                _ => {
                    self.const_locals.remove(name.as_str());
                }
            },
            AstNode::If { .. }
            | AstNode::Call { .. }
            | AstNode::DomainCall { .. }
            | AstNode::Pipeline { .. }
            | AstNode::FunctionDef { .. }
            | AstNode::ImplDef { .. }
            | AstNode::TypeDef { .. }
            | AstNode::Return { .. }
            | AstNode::Break { .. }
            | AstNode::Continue { .. } => self.const_locals.clear(),
            _ => {}
        }
        self.node_inner(node);
        // Control flow may have taken a branch that skipped an assignment, so
        // nothing survives it. `while`/`for` record their own folded results.
        if matches!(
            node,
            AstNode::If { .. }
                | AstNode::While { .. }
                | AstNode::For { .. }
                | AstNode::Call { .. }
                | AstNode::DomainCall { .. }
                | AstNode::Pipeline { .. }
                | AstNode::FunctionDef { .. }
                | AstNode::ImplDef { .. }
                | AstNode::TypeDef { .. }
                | AstNode::Return { .. }
                | AstNode::Break { .. }
                | AstNode::Continue { .. }
        ) {
            self.const_locals.clear();
        }
    }

    fn node_inner(&mut self, node: &AstNode) {
        match node {
            AstNode::Assign { name, value, .. } => {
                let slot = self.local(name);
                // Fast in-place array push: $arr = $arr + [elem]
                if let AstNode::BinaryOp {
                    left,
                    op: BinaryOperator::Add,
                    right,
                    ..
                } = value.as_ref()
                    && let AstNode::Ident { name: ln, .. } = left.as_ref()
                    && ln == name
                    && let AstNode::ArrayLit { elements, .. } = right.as_ref()
                    && elements.len() == 1
                {
                    if self.locals.len() < 200 && self.can_emit_to_slot(&elements[0]) {
                        let elem_slot = self.emit_expr_to_slot(&elements[0], 0);
                        self.emit(Instr::a2(OP_ARRAY_PUSH_LOCAL, slot, elem_slot as u16));
                        return;
                    }
                    self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                    self.node(&elements[0]);
                    self.emit(Instr::none(OP_ARRAY_PUSH));
                    self.emit(Instr::a1(OP_STORE_LOCAL, slot));
                    return;
                }
                // Try recursive RHS fusion for nested binary expressions
                // e.g., $zr = $zr*$zr - $zi*$zi + $cr
                if let AstNode::BinaryOp { .. } = value.as_ref() {
                    if self.try_emit_binary_fused(slot, value.as_ref()) {
                        return;
                    }
                }
                // Direct RHS fusion for array index and field access
                if self.locals.len() < 200 && self.can_emit_to_slot(value.as_ref()) {
                    match value.as_ref() {
                        AstNode::Index { target, index, .. } => {
                            let t_slot = self.emit_expr_to_slot(target, 0);
                            // `$obj["literal"]` is a field read with an
                            // interned key: no need to materialize the key.
                            if let AstNode::Str { value, .. } = index.as_ref() {
                                let field_idx = self.intern(value);
                                // Only emit fused opcode when operands fit.
                                if let (Ok(fi16), Ok(t8)) =
                                    (u16::try_from(field_idx), u8::try_from(t_slot))
                                {
                                    self.emit(Instr::make(
                                        OP_GET_FIELD_LOCAL_STORE,
                                        slot,
                                        fi16,
                                        t8,
                                    ));
                                    return;
                                }
                            }
                            let i_slot = self.emit_expr_to_slot(index, 1);
                            // Only emit fused opcode when operands fit.
                            if let (Ok(is16), Ok(t8)) =
                                (u16::try_from(i_slot), u8::try_from(t_slot))
                            {
                                self.emit(Instr::make(
                                    OP_GET_INDEX_LOCAL_LOCAL_STORE,
                                    slot,
                                    is16,
                                    t8,
                                ));
                                return;
                            }
                        }
                        AstNode::FieldAccess { target, field, .. } => {
                            let t_slot = self.emit_expr_to_slot(target, 0);
                            let field_idx = self.intern(field);
                            // Only emit fused opcode when operands fit.
                            if let (Ok(fi16), Ok(t8)) =
                                (u16::try_from(field_idx), u8::try_from(t_slot))
                            {
                                self.emit(Instr::make(OP_GET_FIELD_LOCAL_STORE, slot, fi16, t8));
                                return;
                            }
                        }
                        _ => {}
                    }
                }
                // Try to fuse hot patterns: $x = $x + 1, $x = $x - 1, $x = $x + c, $a = $a + $b
                if let AstNode::BinaryOp {
                    left, op, right, ..
                } = value.as_ref()
                {
                    if let AstNode::Ident {
                        name: left_name, ..
                    } = left.as_ref()
                    {
                        if left_name == name {
                            // $x = $x op something
                            match op {
                                BinaryOperator::Add => {
                                    if let AstNode::Int { value: 1, .. } = right.as_ref() {
                                        // $x = $x + 1 â†’ OP_INC_LOCAL
                                        self.emit(Instr::a1(OP_INC_LOCAL, slot));
                                        return;
                                    } else if let AstNode::Int { value: c, .. } = right.as_ref() {
                                        // $x = $x + c â†’ OP_INC_LOCAL_INT
                                        self.emit(Instr::make(
                                            OP_INC_LOCAL_INT,
                                            slot,
                                            *c as u16,
                                            0,
                                        ));
                                        return;
                                    } else if let AstNode::Ident {
                                        name: right_name, ..
                                    } = right.as_ref()
                                    {
                                        // $x = $x + $y â†’ OP_ADD_LOCAL_STORE (direct local-to-local)
                                        let right_slot = self.local(right_name);
                                        // Only emit fused opcode when slot fits.
                                        if let Ok(rs16) = u16::try_from(right_slot) {
                                            self.emit(Instr::a2(OP_ADD_LOCAL_STORE, slot, rs16));
                                            return;
                                        }
                                    }
                                }
                                BinaryOperator::Sub => {
                                    if let AstNode::Int { value: 1, .. } = right.as_ref() {
                                        // $x = $x - 1 â†’ OP_DEC_LOCAL
                                        self.emit(Instr::a1(OP_DEC_LOCAL, slot));
                                        return;
                                    } else if let AstNode::Ident {
                                        name: right_name, ..
                                    } = right.as_ref()
                                    {
                                        // $x = $x - $y â†’ OP_SUB_LOCAL_STORE
                                        let right_slot = self.local(right_name);
                                        // Only emit fused opcode when slot fits.
                                        if let Ok(rs16) = u16::try_from(right_slot) {
                                            self.emit(Instr::a2(OP_SUB_LOCAL_STORE, slot, rs16));
                                            return;
                                        }
                                    }
                                }
                                BinaryOperator::Mul => {
                                    if let AstNode::Int { value: c, .. } = right.as_ref() {
                                        // $x = $x * c â†’ OP_MUL_INT_LOCAL
                                        // Only emit when constant fits in u16.
                                        if let Ok(c16) = u16::try_from(*c) {
                                            self.emit(Instr::a2(OP_MUL_INT_LOCAL, slot, c16));
                                            return;
                                        }
                                    } else if let AstNode::Ident {
                                        name: right_name, ..
                                    } = right.as_ref()
                                    {
                                        // $x = $x * $y â†’ OP_MUL_LOCAL_STORE
                                        let right_slot = self.local(right_name);
                                        // Only emit fused opcode when slot fits.
                                        if let Ok(rs16) = u16::try_from(right_slot) {
                                            self.emit(Instr::a2(OP_MUL_LOCAL_STORE, slot, rs16));
                                            return;
                                        }
                                    }
                                }
                                BinaryOperator::Div => {
                                    if let AstNode::Int { value: c, .. } = right.as_ref() {
                                        // $x = $x / c â†’ handled by general path for now
                                        let _ = c;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    // NEW: $z = $a op $b (both operands are distinct locals)
                    if let (AstNode::Ident { name: ln, .. }, AstNode::Ident { name: rn, .. }) =
                        (left.as_ref(), right.as_ref())
                    {
                        let lslot = self.local(ln);
                        let rslot = self.local(rn);
                        // Only emit fused opcode when both operands fit.
                        if let (Ok(l8), Ok(r16)) = (u8::try_from(lslot), u16::try_from(rslot)) {
                            if let Some(opc) = local_local_store_opcode(*op) {
                                self.emit(Instr::make(opc, slot, r16, l8));
                                return;
                            }
                        }
                    }
                }
                self.node(value);
                self.emit(Instr::a1(OP_STORE_LOCAL, slot));
            }
            AstNode::Return { value, .. } => {
                // Tail-call optimization: `return foo(args)`.
                if let Some(AstNode::Call { callee, args, .. }) = value.as_deref() {
                    if let AstNode::Ident { name, .. } = callee.as_ref() {
                        // A `$`-variable callee is a value, not a global name:
                        // load it and use the value tail call.
                        if let Some(slot) =
                            self.locals.iter().position(|n| n.as_ref() == name.as_str())
                        {
                            self.emit(Instr::a1(OP_LOAD_LOCAL, slot as u32));
                            let n = self.emit_call_args(args);
                            self.emit(Instr::a2(OP_TAIL_CALL, n as u32, 0));
                            return;
                        }
                        // Emit OP_TAIL_CALL_NAME: fused tail call with embedded name
                        let i = self.intern(name);
                        if let Ok(name16) = u16::try_from(i) {
                            let n = self.emit_call_args(args);
                            self.emit(Instr::make(OP_TAIL_CALL_NAME, n as u32, name16, 0));
                        } else {
                            self.emit(Instr::a1(OP_PUSH_STR, i));
                            let n = self.emit_call_args(args);
                            self.emit(Instr::a2(OP_TAIL_CALL, n as u32, 0));
                        }
                        return;
                    }
                }
                if let Some(v) = value {
                    self.node(v);
                } else {
                    self.emit(Instr::none(OP_PUSH_NIL));
                }
                self.emit(Instr::none(OP_RETURN));
            }
            AstNode::Break { .. } => self.emit_break(),
            AstNode::Continue { .. } => self.emit_continue(),

            AstNode::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                // `if $x < N` / `if $x == N` fuse the compare and the branch
                // into one dispatch. The branch target goes in the wide table.
                let fused = match condition.as_ref() {
                    AstNode::BinaryOp {
                        left, op, right, ..
                    } => match (left.as_ref(), right.as_ref()) {
                        (AstNode::Ident { name, .. }, AstNode::Int { value, .. })
                            if !self.is_global(name)
                                && *value >= 0
                                && *value <= u16::MAX as i64 =>
                        {
                            let slot = self
                                .locals
                                .iter()
                                .position(|n| &**n == name)
                                .map(|i| i as u32);
                            slot.and_then(|slot| match op {
                                BinaryOperator::Lt => Some((OP_IF_LT_INT, slot, *value as u16)),
                                BinaryOperator::Eq => Some((OP_IF_EQ_INT, slot, *value as u16)),
                                _ => None,
                            })
                        }
                        _ => None,
                    },
                    _ => None,
                };

                if let Some((opc, slot, limit)) = fused
                    && self.wide.len() < 256
                {
                    let widx = self.wide(0) as u8;
                    self.emit(Instr::make(opc, slot, limit, widx));
                    for s in then_branch {
                        self.node(s);
                    }
                    if let Some(e) = else_branch {
                        let j2 = self.ip();
                        self.emit(Instr::a1(OP_JUMP, 0));
                        let t = self.ip();
                        self.wide[widx as usize] = t as i64;
                        for s in e {
                            self.node(s);
                        }
                        let end = self.ip();
                        self.patch_jump(j2, end);
                    } else {
                        let end = self.ip();
                        self.wide[widx as usize] = end as i64;
                    }
                    return;
                }

                self.node(condition);
                let j = self.ip();
                self.emit(Instr::a1(OP_JUMP_IF_FALSE, 0));
                for s in then_branch {
                    self.node(s);
                }
                if let Some(e) = else_branch {
                    let j2 = self.ip();
                    self.emit(Instr::a1(OP_JUMP, 0));
                    let t = self.ip();
                    self.patch_jump(j, t);
                    for s in e {
                        self.node(s);
                    }
                    let end = self.ip();
                    self.patch_jump(j2, end);
                } else {
                    let end = self.ip();
                    self.patch_jump(j, end);
                }
            }

            AstNode::For {
                variable,
                iterable,
                body,
                ..
            } => {
                let entry = core::mem::take(&mut self.const_locals);
                if let AstNode::Range { start, end, .. } = iterable.as_ref() {
                    // Check if start and end are integer constants â€” if so, use OP_FOR_NUMBER_LOOP
                    let start_val = match start.as_ref().map(|n| n.as_ref()) {
                        Some(AstNode::Int { value, .. }) => Some(*value),
                        Some(_) => None,
                        None => Some(0),
                    };
                    let end_val = match end.as_ref().map(|n| n.as_ref()) {
                        Some(AstNode::Int { value, .. }) => Some(*value),
                        Some(_) => None,
                        None => Some(0),
                    };

                    if let (Some(start_i), Some(end_i)) = (start_val, end_val) {
                        if self.fold_for_sum(&entry, variable, start_i, end_i, body) {
                            return;
                        }
                        // Use single-opcode FOR_NUMBER_LOOP (like Luau's FORNLOOP)
                        // Runtime expects: a = (var_slot << 16) | start_wide_idx, b = end_wide_idx
                        let var = self.local(variable);
                        let start_minus_1 = self.wide(start_i.wrapping_sub(1));
                        let end_wide = self.wide(end_i);
                        let exit_wide = self.wide(0);
                        // Check that var_slot and start_wide_idx fit in packed operand a
                        if var <= 0xFFFF && start_minus_1 <= 0xFFFF {
                            self.emit(Instr::a1(OP_PUSH_INT_WIDE, start_minus_1));
                            self.emit(Instr::a1(OP_STORE_LOCAL, var));
                            let loop_start = self.ip();
                            let packed_a = (var << 16) | start_minus_1;
                            self.emit(Instr::make(
                                OP_FOR_NUMBER_LOOP,
                                packed_a,
                                end_wide as u16,
                                exit_wide as u8,
                            ));
                            // `continue` re-enters the header, which advances the
                            // counter and re-tests it.
                            self.begin_loop(loop_start);
                            for s in body {
                                self.node(s);
                            }
                            // Jump back to FOR_NUMBER_LOOP
                            self.emit(Instr::a1(OP_JUMP, loop_start));
                            let exit_ip = self.ip();
                            self.wide[exit_wide as usize] = exit_ip as i64;
                            self.end_loop(exit_ip);
                        } else {
                            // Overflow: fall through to FOR_RANGE_START/NEXT path below
                        }
                    } else {
                        // Fallback: use FOR_RANGE_START/NEXT for non-constant ranges
                        if let Some(s) = start {
                            self.node(s);
                        } else {
                            self.emit(Instr::a1(OP_PUSH_INT, 0));
                        }
                        if let Some(e) = end {
                            self.node(e);
                        } else {
                            self.emit(Instr::a1(OP_PUSH_INT, 0));
                        }
                        self.emit(Instr::none(OP_FOR_RANGE_START));
                        let start_ip = self.ip();
                        let next = self.ip();
                        self.emit(Instr::a1(OP_FOR_RANGE_NEXT, 0));
                        let var = self.local(variable);
                        self.emit(Instr::a1(OP_STORE_LOCAL, var));
                        self.begin_loop(start_ip);
                        for s in body {
                            self.node(s);
                        }
                        self.emit(Instr::a1(OP_JUMP, start_ip));
                        let end = self.ip();
                        self.patch_jump(next, end);
                        self.end_loop(end);
                    }
                } else {
                    self.node(iterable);
                    self.emit(Instr::none(OP_FOR_ITER_START));
                    let start_ip = self.ip();
                    let next = self.ip();
                    self.emit(Instr::a1(OP_FOR_ITER_NEXT, 0));
                    let var = self.local(variable);
                    self.emit(Instr::a1(OP_STORE_LOCAL, var));
                    self.begin_loop(start_ip);
                    for s in body {
                        self.node(s);
                    }
                    self.emit(Instr::a1(OP_JUMP, start_ip));
                    let end = self.ip();
                    self.patch_jump(next, end);
                    self.end_loop(end);
                }
            }

            AstNode::While {
                condition, body, ..
            } => {
                // The body runs repeatedly, so only values known before the loop
                // describe the first iteration. The fold consumes them; nothing
                // is known inside the body.
                let entry = core::mem::take(&mut self.const_locals);
                // Try to fuse: while $i < N { ... } â†’ OP_WHILE_LT_INT
                // Or: while $a < $b { ... } â†’ OP_WHILE_LT_LOCAL
                match condition.as_ref() {
                    AstNode::BinaryOp {
                        left,
                        op: BinaryOperator::Lt,
                        right,
                        ..
                    } => {
                        if let AstNode::Ident {
                            name: left_name, ..
                        } = left.as_ref()
                        {
                            if let AstNode::Int { value: limit, .. } = right.as_ref()
                                && (0..=i64::from(u16::MAX)).contains(limit)
                            {
                                if self.fold_while_sum(&entry, left_name, *limit, body) {
                                    return;
                                }
                                let slot = self.local(left_name);
                                let exit_wide = self.wide(0);
                                let loop_start = self.ip();
                                self.emit(Instr::make(
                                    OP_WHILE_LT_INT,
                                    slot,
                                    *limit as u16,
                                    exit_wide as u8,
                                ));
                                self.begin_loop(loop_start);
                                for s in body {
                                    self.node(s);
                                }
                                self.emit(Instr::a1(OP_JUMP, loop_start));
                                let exit_ip = self.ip();
                                self.wide[exit_wide as usize] = exit_ip as i64;
                                self.end_loop(exit_ip);
                                return;
                            } else if let AstNode::Int { value: limit, .. } = right.as_ref() {
                                // Limits above `u16::MAX` still fuse: the limit
                                // moves into `wide_operands` (operand `b`).
                                if self.fold_while_sum(&entry, left_name, *limit, body) {
                                    return;
                                }
                                let slot = self.local(left_name);
                                let limit_wide = self.wide(*limit);
                                let exit_wide = self.wide(0);
                                if let (Ok(limit_idx), Ok(exit_idx)) =
                                    (u16::try_from(limit_wide), u8::try_from(exit_wide))
                                {
                                    let loop_start = self.ip();
                                    self.emit(Instr::make(
                                        OP_WHILE_LT_INT_WIDE,
                                        slot,
                                        limit_idx,
                                        exit_idx,
                                    ));
                                    self.begin_loop(loop_start);
                                    for s in body {
                                        self.node(s);
                                    }
                                    self.emit(Instr::a1(OP_JUMP, loop_start));
                                    let exit_ip = self.ip();
                                    self.wide[exit_wide as usize] = exit_ip as i64;
                                    self.end_loop(exit_ip);
                                    return;
                                }
                            } else if let AstNode::Ident {
                                name: right_name, ..
                            } = right.as_ref()
                            {
                                let left_slot = self.local(left_name);
                                let right_slot = self.local(right_name);
                                let exit_wide = self.wide(0);
                                let loop_start = self.ip();
                                self.emit(Instr::make(
                                    OP_WHILE_LT_LOCAL,
                                    left_slot,
                                    right_slot as u16,
                                    exit_wide as u8,
                                ));
                                self.begin_loop(loop_start);
                                for s in body {
                                    self.node(s);
                                }
                                self.emit(Instr::a1(OP_JUMP, loop_start));
                                let exit_ip = self.ip();
                                self.wide[exit_wide as usize] = exit_ip as i64;
                                self.end_loop(exit_ip);
                                return;
                            }
                        }
                    }
                    _ => {}
                };

                // Fallback: generic while loop
                let start = self.ip();
                self.node(condition);
                let j = self.ip();
                self.emit(Instr::a1(OP_JUMP_IF_FALSE, 0));
                self.begin_loop(start);
                for s in body {
                    self.node(s);
                }
                self.emit(Instr::a1(OP_JUMP, start));
                let end = self.ip();
                self.patch_jump(j, end);
                self.end_loop(end);
            }

            AstNode::FunctionDef {
                body, name, params, ..
            } => {
                let mut sub = self
                    .body_compiler()
                    .with_inline_map(core::mem::take(&mut self.inline_map));
                for p in params {
                    sub.local(&p.name);
                }
                // Names assigned anywhere in the body are locals for the whole
                // body, so they shadow same-named top-level variables.
                let mut reads = Vec::new();
                let mut writes = Vec::new();
                for s in body {
                    scan_vars(s, &mut reads, &mut writes);
                }
                for w in &writes {
                    sub.local(w);
                }
                for s in body {
                    sub.node(s);
                }
                sub.emit(Instr::none(OP_RETURN));
                sub.emit(Instr::none(OP_HALT));

                let bc = sub.compile(&[]);
                // Restore inline_map for sibling nodes
                self.inline_map = core::mem::take(&mut sub.inline_map);
                let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                let f = FuncValue {
                    name: Arc::from(name.as_str()),
                    bytecode: Some(Arc::new(bc)),
                    params: Arc::from(param_names),
                    closure_captures: Arc::from(Vec::<String>::new()),
                    closure_env: smallvec::SmallVec::new(),
                    uses_argv: sub.uses_argv,
                };
                self.functions.push((name.as_str().into(), Arc::new(f)));
            }

            AstNode::Call { callee, args, .. } => {
                // A bare `$f` statement: call the value when it holds a function.
                // (`$x` holding anything else is left on the stack as before.)
                if args.is_empty() {
                    if let AstNode::Ident { name, .. } = callee.as_ref() {
                        let local = self
                            .locals
                            .iter()
                            .position(|n| n.as_ref() == name.as_str())
                            .map(|i| i as u32);
                        if let Some(slot) = local {
                            self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                            self.emit(Instr::none(OP_CALL_VALUE_ZERO));
                            return;
                        }
                        if let Some(slot) = self.root_read_slot(name) {
                            self.emit(Instr::a1(OP_LOAD_ROOT, slot));
                            self.emit(Instr::none(OP_CALL_VALUE_ZERO));
                            return;
                        }
                    }
                }
                // Try to inline: if callee is a plain identifier matching an inlineable function
                if let AstNode::Ident { name, .. } = callee.as_ref() {
                    // A `$`-variable with the same name shadows the global function,
                    // so the call must stay dynamic.
                    let shadowed = self.locals.iter().any(|n| n.as_ref() == name.as_str());
                    // Clone the inline body out to avoid borrow conflicts with self
                    let inline_body = if shadowed {
                        None
                    } else {
                        self.inline_map
                            .get(name)
                            .map(|f| (f.params.clone(), f.body.clone()))
                    };

                    if let Some((params, body)) = &inline_body {
                        // Criterion: arg count must match
                        if args.len() == params.len()
                            // Named arguments are not positional; inlining would
                            // substitute them into the wrong parameters.
                            && args.iter().all(|a| a.name.is_none())
                            // Criterion 4: every arg must be simple
                            && args.iter().all(|a| is_simple_arg(&a.value))
                            // Recursion guard: do not inline if already on the stack
                            && !self.inline_stack.contains(name)
                            // Depth limit
                            && self.inline_stack.len() < 8
                        {
                            // Build substitution map: param -> arg expression
                            let subs: Vec<(String, AstNode)> = params
                                .iter()
                                .cloned()
                                .zip(args.iter().map(|a| a.value.clone()))
                                .collect();

                            match body {
                                InlineBody::Expr(body_expr) => {
                                    // Original single-expression inlining
                                    let body = substitute(body_expr, &subs);
                                    self.node(&body);
                                    return;
                                }
                                InlineBody::Block { assigns, result } => {
                                    // Multi-statement block inlining:
                                    // 1. Create fresh names for body-locals
                                    // 2. Substitute params and body-locals
                                    // 3. Compile each assignment
                                    // 4. Compile the result expression

                                    // Build fresh name mapping for body-locals
                                    let mut fresh_subs = subs.clone();
                                    let mut local_map: Vec<(String, String)> = Vec::new();
                                    for (local_name, _) in assigns {
                                        let fresh = format!(
                                            "__xcl_inl_{}_{}",
                                            self.inline_counter, local_name
                                        );
                                        self.inline_counter += 1;
                                        let old_name = local_name.clone();
                                        local_map.push((old_name.clone(), fresh.clone()));
                                        fresh_subs.push((
                                            old_name,
                                            AstNode::Ident {
                                                name: fresh.clone(),
                                                span: Span::empty(),
                                            },
                                        ));
                                    }

                                    // Push this function onto the inline stack
                                    self.inline_stack.push(name.clone());

                                    // Compile each assignment with substituted values
                                    for (local_name, value_expr) in assigns {
                                        let substituted_value = substitute(value_expr, &fresh_subs);
                                        // Find the fresh name for this local
                                        let fresh_name = local_map
                                            .iter()
                                            .find(|(old, _)| *old == *local_name)
                                            .map(|(_, fresh)| fresh.as_str())
                                            .unwrap_or(local_name.as_str());
                                        let slot = self.local(fresh_name);
                                        // Compile: $fresh = substituted_value
                                        self.node(&substituted_value);
                                        self.emit(Instr::a1(OP_STORE_LOCAL, slot));
                                    }

                                    // Compile the result expression
                                    let substituted_result = substitute(result, &fresh_subs);
                                    self.node(&substituted_result);

                                    // Pop the inline stack
                                    self.inline_stack.pop();

                                    return;
                                }
                            }
                        }
                    }
                }

                // Build full dotted name from nested FieldAccess chains.
                // e.g., net.tcp.connect -> "net.tcp.connect"
                //       io.read -> "io.read"
                //       $r.map_err -> "$r.map_err"
                let full_name = collect_dotted_name(callee.as_ref());
                if let Some(name) = full_name {
                    // Check if the base identifier is a local variable AND there's a dot (method call)
                    let parts: Vec<&str> = name.split('.').collect();
                    let base = parts.first().copied().unwrap_or(&name);
                    // Strip $ prefix from base for local variable lookup
                    let base_unprefixed = base.strip_prefix('$').unwrap_or(base);
                    let is_local = self.locals.iter().any(|n| n.as_ref() == base_unprefixed);
                    let root_slot = if is_local {
                        None
                    } else {
                        self.root_read_slot(base_unprefixed)
                    };
                    if is_local && parts.len() > 1 {
                        // It's a method call on a local value: $x.method()
                        // Compile as OP_CALL_METHOD: load receiver, push args, CALL_METHOD
                        let method_name = parts.last().copied().unwrap_or(&name);
                        let field_idx = self.intern(method_name);
                        // Load the receiver (the local variable)
                        if let Some(slot) = self
                            .locals
                            .iter()
                            .position(|n| n.as_ref() == base_unprefixed)
                            .map(|i| i as u32)
                        {
                            self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                        }
                        // Push args
                        let n = self.emit_call_args(args);
                        self.emit(Instr::a2(OP_CALL_METHOD, field_idx, n as u16));
                    } else if let Some(slot) = root_slot.filter(|_| parts.len() > 1) {
                        // Method call on a top-level variable read from a
                        // function body: `$r.is_ok`.
                        let method_name = parts.last().copied().unwrap_or(&name);
                        let field_idx = self.intern(method_name);
                        self.emit(Instr::a1(OP_LOAD_ROOT, slot));
                        let n = self.emit_call_args(args);
                        self.emit(Instr::a2(OP_CALL_METHOD, field_idx, n as u16));
                    } else if is_local && parts.len() == 1 {
                        // Single-part local variable call: $f() or $f arg
                        if let Some(slot) = self
                            .locals
                            .iter()
                            .position(|n| **n == *base)
                            .map(|i| i as u32)
                        {
                            self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                        }
                        let n = self.emit_call_args(args);
                        self.emit(Instr::a2(OP_CALL, n as u32, 0));
                    } else {
                        // Bare or dotted name call. Embed the callee name in the
                        // opcode so the name push and the call are one dispatch.
                        // The handler resolves builtins, user functions, and
                        // domain methods exactly like `OP_CALL`.
                        let i = self.intern(&name);
                        if let Ok(name16) = u16::try_from(i) {
                            let n = self.emit_call_args(args);
                            self.emit_named_call(i, name16, n);
                        } else {
                            self.emit(Instr::a1(OP_PUSH_STR, i));
                            let n = self.emit_call_args(args);
                            self.emit(Instr::a2(OP_CALL, n as u32, 0));
                        }
                    }
                } else if let AstNode::Ident { name, .. } = callee.as_ref() {
                    // If the name was originally a $-prefixed variable, load from local
                    // Otherwise push the name string for global/builtin lookup
                    if let Some(slot) = self
                        .locals
                        .iter()
                        .position(|n| **n == *name)
                        .map(|i| i as u32)
                    {
                        self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                        let n = self.emit_call_args(args);
                        self.emit(Instr::a2(OP_CALL, n as u32, 0));
                    } else {
                        // Fused named call: embed the callee name in the opcode
                        let i = self.intern(name);
                        if let Ok(name16) = u16::try_from(i) {
                            let n = self.emit_call_args(args);
                            self.emit_named_call(i, name16, n);
                        } else {
                            self.emit(Instr::a1(OP_PUSH_STR, i));
                            let n = self.emit_call_args(args);
                            self.emit(Instr::a2(OP_CALL, n as u32, 0));
                        }
                    }
                } else {
                    self.node(callee);
                    let n = self.emit_call_args(args);
                    self.emit(Instr::a2(OP_CALL, n as u32, 0));
                }
            }

            AstNode::DomainCall {
                callee,
                args,
                items,
                span,
            } => {
                if items.is_empty() {
                    self.emit(Instr::none(OP_PUSH_NIL));
                    return;
                }
                for (i, item) in items.iter().enumerate() {
                    // Positional args first, then the item value, then the named
                    // map. Item named args replace shared ones with the same key.
                    let mut call_args: Vec<CallArg> =
                        args.iter().filter(|a| a.name.is_none()).cloned().collect();
                    call_args.push(CallArg {
                        value: item.value.clone(),
                        name: None,
                    });
                    for a in args.iter().filter(|a| a.name.is_some()) {
                        let key = a.name.as_ref().expect("named arg");
                        if !item.named.iter().any(|(k, _)| k == key) {
                            call_args.push(a.clone());
                        }
                    }
                    for (key, value) in &item.named {
                        call_args.push(CallArg {
                            value: value.clone(),
                            name: Some(key.clone()),
                        });
                    }
                    let call = AstNode::Call {
                        span: span.clone(),
                        callee: callee.clone(),
                        args: call_args,
                    };
                    self.node(&call);
                    if i + 1 != items.len() {
                        self.emit(Instr::none(OP_POP));
                    }
                }
            }

            AstNode::Int { value, .. } => {
                if (0..=u32::MAX as i64).contains(value) {
                    self.emit(Instr::a1(OP_PUSH_INT, *value as u32));
                } else {
                    let idx = self.wide(*value);
                    self.emit(Instr::a1(OP_PUSH_INT_WIDE, idx));
                }
            }
            AstNode::Float { value, .. } => {
                let idx = self.wide(value.to_bits() as i64);
                self.emit(Instr::a1(OP_PUSH_FLOAT, idx));
            }
            AstNode::Str { value, .. } => {
                let i = self.intern(value);
                self.emit(Instr::a1(OP_PUSH_STR, i));
            }
            AstNode::Bool { value, .. } => {
                self.emit(Instr::none(if *value {
                    OP_PUSH_TRUE
                } else {
                    OP_PUSH_FALSE
                }));
            }
            AstNode::Nil { .. } => self.emit(Instr::none(OP_PUSH_NIL)),

            AstNode::Ident { name, .. } => {
                // Check if this is a registered global (enum variant, type name)
                if self.is_global(name) {
                    let g_slot = self.global(name);
                    self.emit(Instr::a1(OP_LOAD_GLOBAL, g_slot));
                } else if let Some(slot) = self.root_read_slot(name) {
                    self.emit(Instr::a1(OP_LOAD_ROOT, slot));
                } else {
                    let slot = self.local(name);
                    if !self.try_fuse_load_load(slot) {
                        self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                    }
                }
            }

            AstNode::BinaryOp {
                left, op, right, ..
            } => {
                self.node(left);
                self.node(right);
                // Try superinstructions AFTER emitting operands
                if let Some(opc) = local_local_opcode(*op)
                    && self.try_fuse_local_local(opc)
                {
                    return;
                }
                let opcode = binop_opcode(*op);
                self.emit(Instr::none(opcode));
            }

            AstNode::UnaryOp { op, operand, .. } => {
                self.node(operand);
                let opcode = match op {
                    UnaryOperator::Neg => OP_NEG,
                    UnaryOperator::Not => OP_NOT,
                    UnaryOperator::BitNot => OP_BITNOT,
                };
                self.emit(Instr::none(opcode));
            }

            AstNode::ArrayLit { elements, .. } => {
                for e in elements {
                    self.node(e);
                }
                self.emit(Instr::a1(OP_MAKE_ARRAY, elements.len() as u32));
            }

            AstNode::DictLit { entries, .. } => {
                for (k, v) in entries {
                    let i = self.intern(k);
                    self.emit(Instr::a1(OP_PUSH_STR, i));
                    self.node(v);
                }
                self.emit(Instr::a1(OP_MAKE_DICT, entries.len() as u32));
            }

            AstNode::Block { statements, .. } => {
                for s in statements {
                    self.node(s);
                }
            }

            AstNode::Index { target, index, .. } => {
                self.node(target);
                self.node(index);
                self.emit(Instr::none(OP_GET_INDEX));
            }

            AstNode::FieldAccess { target, field, .. } => {
                // A dotted chain rooted at a plain identifier is a domain
                // method: `metadata.all` in expression position means the
                // same as the statement form. Without this it compiled to a
                // field read on an uninitialized local named `metadata`.
                // `$var.field`, globals, and type/variant names stay field
                // reads.
                if let Some(name) = self.zero_arg_domain_call(node) {
                    let i = self.intern(&name);
                    if let Ok(name16) = u16::try_from(i) {
                        self.emit_named_call(i, name16, 0);
                    } else {
                        self.emit(Instr::a1(OP_PUSH_STR, i));
                        self.emit(Instr::a2(OP_CALL, 0, 0));
                    }
                    return;
                }
                self.node(target);
                let i = self.intern(field);
                self.emit(Instr::a1(OP_GET_FIELD, i));
            }

            AstNode::Range { start, end, .. } => {
                if let Some(s) = start {
                    self.node(s);
                } else {
                    self.emit(Instr::none(OP_PUSH_NIL));
                }
                if let Some(e) = end {
                    self.node(e);
                } else {
                    self.emit(Instr::none(OP_PUSH_NIL));
                }
                self.emit(Instr::none(OP_MAKE_RANGE));
            }

            AstNode::Lambda { params, body, .. } => {
                // Resolve the body's free names against the enclosing scope:
                // values that live in this frame are captured by value.
                let mut reads = Vec::new();
                let mut writes = Vec::new();
                scan_vars(body, &mut reads, &mut writes);
                let mut capture_names: Vec<String> = Vec::new();
                let mut capture_slots: Vec<u32> = Vec::new();
                for name in &reads {
                    if params.contains(name) || writes.iter().any(|w| w == name) {
                        continue;
                    }
                    if let Some(&slot) = self.local_index.get(name.as_str()) {
                        capture_names.push(name.clone());
                        capture_slots.push(slot);
                    }
                    // Top-level variables are read through `OP_LOAD_ROOT` and
                    // do not need to be captured.
                }

                let mut sub = self.body_compiler();
                for p in params {
                    sub.local(p);
                }
                // Captures occupy the slots right after the parameters; the VM
                // writes their values there when the closure is entered.
                for c in &capture_names {
                    sub.local(c);
                }
                for w in &writes {
                    sub.local(w);
                }
                sub.node(body);
                sub.emit(Instr::none(OP_RETURN));
                sub.emit(Instr::none(OP_HALT));

                let bc = sub.compile(&[]);
                let f = FuncValue {
                    name: Arc::from("lambda"),
                    bytecode: Some(Arc::new(bc)),
                    params: Arc::from(params.clone()),
                    closure_captures: Arc::from(capture_names),
                    closure_env: smallvec::SmallVec::new(),
                    uses_argv: sub.uses_argv,
                };
                let idx = self.closures.len() as u32;
                self.closures.push(f);
                // Push the captured values, then build the closure from them.
                for slot in capture_slots {
                    self.emit(Instr::a1(OP_LOAD_LOCAL, slot));
                }
                self.emit(Instr::a1(OP_LOAD_LAMBDA, idx));
            }

            AstNode::Pipeline { input, stages, .. } => {
                // A pipeline passes the accumulator as the first argument. The
                // stage is a named callee, so `OP_CALL_NAME` is the direct
                // encoding: `[acc, arg]` plus the name in the opcode.
                self.node(input);
                let depth = self.pipeline_depth;
                self.pipeline_depth += 1;
                for stage in stages {
                    if let Some(a) = &stage.arg {
                        self.node(a);
                    } else {
                        self.emit(Instr::none(OP_PUSH_NIL));
                    }
                    let i = self.intern(&stage.name);
                    if let Ok(name16) = u16::try_from(i) {
                        self.emit_named_call(i, name16, 2);
                    } else {
                        // String-table overflow: rebuild `[callee, acc, arg]`
                        // explicitly (more than 65535 distinct strings).
                        let base = PIPE_TEMP_BASE + depth * 2;
                        let arg = self.temp_slot(base);
                        self.emit(Instr::a1(OP_STORE_LOCAL, arg));
                        let acc = self.temp_slot(base + 1);
                        self.emit(Instr::a1(OP_STORE_LOCAL, acc));
                        self.emit(Instr::a1(OP_PUSH_STR, i));
                        self.emit(Instr::a1(OP_LOAD_LOCAL, acc));
                        self.emit(Instr::a1(OP_LOAD_LOCAL, arg));
                        self.emit(Instr::a2(OP_CALL, 2, 0));
                    }
                }
                self.pipeline_depth = depth;
            }

            // Type definition: compile body (fields + methods), register type name as a global
            AstNode::TypeDef {
                name, kind, body, ..
            } => {
                // Compile any methods in the body
                for s in body {
                    if let AstNode::FunctionDef { .. } = s {
                        self.node(s);
                    }
                }
                // Register the type name as a global
                let type_idx = self.intern(name);
                self.emit(Instr::a1(OP_PUSH_STR, type_idx));
                let g_slot = self.global(name);
                self.emit(Instr::a1(OP_STORE_GLOBAL, g_slot));
                // For sum types, register each variant as a global string constant
                if let TypeKind::Sum { variants } = kind {
                    for v in variants {
                        let v_idx = self.intern(&v.name);
                        self.emit(Instr::a1(OP_PUSH_STR, v_idx));
                        let vg_slot = self.global(&v.name);
                        self.emit(Instr::a1(OP_STORE_GLOBAL, vg_slot));
                    }
                }
            }

            // Impl block: compile methods, associate with type
            AstNode::ImplDef {
                type_name,
                trait_name,
                body,
                ..
            } => {
                // Compile all methods in the impl body
                for s in body {
                    if let AstNode::FunctionDef { .. } = s {
                        self.node(s);
                    }
                }
                // Store the impl relationship as a global marker
                let impl_key = format!("__impl_{}_{}", type_name, trait_name);
                let idx = self.intern(&impl_key);
                self.emit(Instr::a1(OP_PUSH_STR, idx));
                self.emit(Instr::none(OP_PUSH_TRUE));
            }

            // Type test: $x is TypeName
            AstNode::TypeTest {
                value, type_name, ..
            } => {
                self.node(value);
                let type_idx = self.intern(type_name);
                self.emit(Instr::a1(OP_PUSH_STR, type_idx));
                self.emit(Instr::none(OP_TYPE_TEST));
            }

            // Type cast: $x as TypeName
            AstNode::TypeCast {
                value, type_name, ..
            } => {
                self.node(value);
                let type_idx = self.intern(type_name);
                self.emit(Instr::a1(OP_PUSH_STR, type_idx));
                self.emit(Instr::none(OP_TYPE_CAST));
            }

            AstNode::SpecialVar { kind, index, .. } => {
                use crate::runtime::opcode::special_var_kind as sv;
                // Only these forms need the positional argument vector. `$0`,
                // `$self`, and `$super` are served by the VM / `Frame::arg0`.
                if matches!(
                    kind,
                    SpecialVarKind::Positional
                        | SpecialVarKind::ArgCount
                        | SpecialVarKind::ArgsArray
                        | SpecialVarKind::ArgsJoined
                ) {
                    self.uses_argv = true;
                }
                let kind_code = match kind {
                    SpecialVarKind::ScriptName => sv::SCRIPT_NAME,
                    SpecialVarKind::Positional => sv::POSITIONAL,
                    SpecialVarKind::ArgCount => sv::ARG_COUNT,
                    SpecialVarKind::ArgsArray => sv::ARGS_ARRAY,
                    SpecialVarKind::ArgsJoined => sv::ARGS_JOINED,
                    SpecialVarKind::SelfValue => sv::SELF,
                    SpecialVarKind::SuperValue => sv::SUPER,
                };
                // a = positional index (32-bit), c = kind (8-bit)
                self.emit(Instr::make(OP_SPECIAL_VAR, *index, 0, kind_code));
            }
        }
    }
}
