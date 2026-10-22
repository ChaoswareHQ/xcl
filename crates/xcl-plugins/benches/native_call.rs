//! Native-call dispatch: `OP_CALL_HOT` vs `OP_CALL_NAME`, against Luau.
//!
//! Each variant runs the same loop with exactly one call in the body, and the
//! bytecode is compiled once outside the timed region so the measurement is the
//! call itself rather than compilation.
//!
//! The point of the group is the spread between `xcl-inline` (no call),
//! `xcl-hot` (a native the VM gave a slot), `xcl-named` (the same native reached
//! by name), and `xcl-user` (a script function). If `xcl-hot` ever approaches
//! `xcl-named`, the slot table has stopped being consulted.

use std::hint::black_box;

use criterion::{BenchmarkGroup, Criterion, criterion_group, criterion_main};
use xcl_vm::{Bytecode, VM, bytecode};

const ITERATIONS: u32 = 2000;

/// Runs one builtin call per iteration through the hot native slot.
const HOT: &str = r#"
$r = util.ok 1
$i = 0
$s = 0
while $i < 2000 {
    if result.is_ok $r { $s = $s + 1 }
    $i = $i + 1
}
$s
"#;

/// The same call to the same native, reached by name.
const NAMED: &str = HOT;

/// A script function call, for scale: frame entry, `function_by_name`, return.
const USER: &str = r#"
ok($v) { return $v != none }
$r = util.ok 1
$i = 0
$s = 0
while $i < 2000 {
    if ok $r { $s = $s + 1 }
    $i = $i + 1
}
$s
"#;

/// The loop with the call replaced by a local read: the floor.
const INLINE: &str = r#"
$i = 0
$s = 0
while $i < 2000 {
    if true { $s = $s + 1 }
    $i = $i + 1
}
$s
"#;

fn compile(src: &str) -> Bytecode {
    let tokens = xcl_vm::lexer::lex(src, "<bench>");
    let ast = xcl_vm::parser::parse(&tokens, src);
    bytecode::compile(&ast)
}

fn compile_hot(src: &str, vm: &VM) -> Bytecode {
    let tokens = xcl_vm::lexer::lex(src, "<bench>");
    let ast = xcl_vm::parser::parse(&tokens, src);
    bytecode::compile_hot(&ast, vm.hot_table())
}

/// Same loop at 2M iterations: the counter limit no longer fits in the
/// instruction, so the latch reads it from the wide-operand table every pass.
const HOT_WIDE: &str = r#"
$r = util.ok 1
$i = 0
$s = 0
while $i < 2000000 {
    if result.is_ok $r { $s = $s + 1 }
    $i = $i + 1
}
$s
"#;

fn bench_native_call(c: &mut Criterion) {
    let mut group: BenchmarkGroup<_> = c.benchmark_group("native_call_2k");

    let mut vm = xcl_plugins::new_vm();
    let hot = compile_hot(HOT, &vm);
    let named = compile(NAMED);
    let user = compile(USER);
    let inline = compile(INLINE);

    assert_eq!(vm.run(&hot).as_i48(), i64::from(ITERATIONS));
    assert_eq!(vm.run(&named).as_i48(), i64::from(ITERATIONS));
    assert_eq!(vm.run(&user).as_i48(), i64::from(ITERATIONS));
    assert_eq!(vm.run(&inline).as_i48(), i64::from(ITERATIONS));

    assert!(
        hot.instructions
            .iter()
            .any(|i| i.op() == bytecode::OP_CALL_HOT),
        "the hot variant must use the slot"
    );
    assert!(
        named
            .instructions
            .iter()
            .all(|i| i.op() != bytecode::OP_CALL_HOT),
        "the named variant must not use the slot"
    );

    group.bench_function("xcl-inline", |b| {
        b.iter(|| black_box(vm.run(&inline)).as_i48())
    });
    // Same loop at 2M iterations: the limit no longer fits in the instruction,
    // so the latch reads it from the wide-operand table on every pass.
    let hot_wide = compile_hot(HOT_WIDE, &vm);
    assert_eq!(vm.run(&hot_wide).as_i48(), 2_000_000);
    group.bench_function("xcl-hot-wide-2m", |b| {
        b.iter(|| black_box(vm.run(&hot_wide)).as_i48())
    });
    group.bench_function("xcl-hot", |b| b.iter(|| black_box(vm.run(&hot)).as_i48()));
    group.bench_function("xcl-named", |b| {
        b.iter(|| black_box(vm.run(&named)).as_i48())
    });
    group.bench_function("xcl-user", |b| b.iter(|| black_box(vm.run(&user)).as_i48()));

    // Luau: same loop, `type(r)` is a C function call per iteration.
    let lua = mlua::Lua::new();
    let chunk = lua
        .load(
            "local r = 1 local i = 0 local s = 0 \
             while i < 2000 do if type(r) == \"number\" then s = s + 1 end i = i + 1 end \
             return s",
        )
        .into_function()
        .expect("luau chunk compiles");
    group.bench_function("luau-builtin", |b| {
        b.iter(|| black_box(chunk.call::<i64>(())).unwrap())
    });

    group.finish();
}

criterion_group!(benches, bench_native_call);
criterion_main!(benches);
