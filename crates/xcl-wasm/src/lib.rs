//! WebAssembly facade for the XCL playground.
//!
//! The page needs four answers, and each export returns one of them as JSON so
//! the client never has to parse prose:
//!
//! | export | returns |
//! | --- | --- |
//! | [`diagnose`] | NDJSON diagnostics, one object per line — the same contract `xcl --lint --error-format=json` emits |
//! | [`decompile`] | `{"ok":bool,"text":string}` — the disassembly listing |
//! | [`program`] | the compiled image: every instruction as a 64-bit word plus `op`/`a`/`b`/`c` |
//! | [`opcode_map`] | the whole 256-byte opcode space: core, growth, `OP_EXT`, hot slots |
//!
//! [`run`] executes the program on the core VM.
//!
//! # What is linked, and what is not
//!
//! This crate depends on `xcl-vm`, `xcl-analysis`, and the decompiler — not on
//! `xcl-plugins`. The plugin layer registers itself through `linkme`
//! distributed slices, which `wasm32-unknown-unknown` does not support (the
//! section symbols are never emitted, so every registry is empty). That is a
//! deliberate trade:
//!
//! * **Works in the browser:** primitives, operators, control flow, functions,
//!   closures, records, arrays, maps, `type`/`super`, linting, disassembly, the
//!   full opcode map, and a three-function prelude (`print`, `println`, `len`).
//! * **Not linked:** std domains (`util.*`, `math.*`, …), `fs`, `io`, `https`,
//!   and plugin opcodes. [`run`] therefore executes core-language programs and
//!   captures the output of the prelude instead of writing to a console.
//!
//! Everything here also compiles for the host, which is how it is tested:
//! outside wasm `#[wasm_bindgen]` emits ordinary Rust functions.
//!
//! # Building
//!
//! ```sh
//! cargo build -p xcl-wasm --target wasm32-unknown-unknown --release
//! wasm-bindgen --target web --out-dir <site>/wwwroot/playground \
//!     target/wasm32-unknown-unknown/release/xcl_wasm.wasm
//! ```

use std::cell::RefCell;

use std::sync::Arc;
use wasm_bindgen::prelude::*;
use xcl_analysis::{LintConfig, SourceMap, check_source, json_string};
use xcl_vm::runtime::opcode::layout;

use xcl_vm::{
    Bytecode, CapSet, FuncValue, Instr, NativeEntry, NativeKind, OpcodeEntry, OpcodeResult,
    ParseError, RawPtr, VM, Value,
};

/// Name used in diagnostics; the playground has no file on disk.
const FILE: &str = "<playground>";

/// Cap on captured console output, so a runaway loop cannot fill the heap with
/// strings before the tab is killed.
const OUTPUT_CAP: usize = 64 * 1024;

thread_local! {
    static OUTPUT: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Crate version, so the page can show what it is running.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Every byte of the opcode space, classified.
///
/// The page renders this as the 128/128 grid: codes `0..=127` are the core
/// range (`opcode`, reserved growth, or `OP_EXT`), and `128..=255` are hot
/// module slots whose index is `op & 0x7f`.
///
/// ```
/// use xcl_wasm::opcode_map;
///
/// let map = opcode_map();
/// assert!(map.contains("\"opExt\":127"));
/// assert_eq!(map.matches("\"hex\":").count(), 256);
/// ```
#[wasm_bindgen]
pub fn opcode_map() -> String {
    let mut out = String::with_capacity(16 * 1024);
    out.push_str("{\"coreFirst\":");
    out.push_str(&layout::CORE_FIRST.to_string());
    out.push_str(",\"coreLast\":");
    out.push_str(&layout::CORE_LAST.to_string());
    out.push_str(",\"opExt\":");
    out.push_str(&layout::OP_EXT.to_string());
    out.push_str(",\"moduleFirst\":");
    out.push_str(&layout::MODULE_FIRST.to_string());
    out.push_str(",\"moduleCount\":");
    out.push_str(&layout::MODULE_COUNT.to_string());
    out.push_str(",\"entries\":[");

    for code in 0..=u8::MAX {
        if code > 0 {
            out.push(',');
        }
        let (kind, name) = classify(code);
        out.push_str("{\"code\":");
        out.push_str(&code.to_string());
        out.push_str(",\"hex\":\"0x");
        out.push_str(&format!("{code:02x}"));
        out.push_str("\",\"kind\":");
        out.push_str(&json_string(kind));
        out.push_str(",\"name\":");
        match name {
            Some(name) => out.push_str(&json_string(name)),
            None => out.push_str("null"),
        }
        out.push_str(",\"slot\":");
        if layout::is_module_slot(code) {
            out.push_str(&layout::module_index(code).to_string());
        } else {
            out.push_str("null");
        }
        out.push('}');
    }
    out.push_str("],\"plugin\":{");
    out.push_str("\"name\":\"example\",\"ops\":[");
    for (index, entry) in EXAMPLE_PLUGIN.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"code\":");
        out.push_str(&entry.code.to_string());
        out.push_str(",\"name\":");
        out.push_str(&json_string(entry.name));
        out.push('}');
    }
    out.push_str("]}}");
    out
}

/// Classifies one opcode code.
fn classify(code: u8) -> (&'static str, Option<&'static str>) {
    if code == layout::OP_EXT {
        return ("ext", xcl_vm::opcode_name(code));
    }
    if layout::is_module_slot(code) {
        // The example plugin below is the only owner of hot slots in this build.
        return ("hot", plugin_name(code));
    }
    match xcl_vm::opcode_name(code) {
        Some(name) => ("core", Some(name)),
        None if layout::CORE_GROWTH.contains(&code) => ("growth", None),
        None => ("unused", None),
    }
}

/// Runs every lint and returns NDJSON, one diagnostic object per line.
///
/// Empty string means the program is clean. Syntax errors are reported in the
/// same stream with the parser's codes (`E0001`…), exactly as the CLI does.
#[wasm_bindgen]
pub fn diagnose(source: &str) -> String {
    let report = check_source(FILE, source, &LintConfig::new());
    let map = SourceMap::new(FILE, source);
    report.render_json(&map)
}

/// Compiles `source` and returns the disassembly.
///
/// ```json
/// {"ok":true,"text":"..."}
/// {"ok":false,"errors":[{"code":"E0001","message":"...","line":1,"col":4}]}
/// ```
#[wasm_bindgen]
pub fn decompile(source: &str) -> String {
    match compile(source) {
        Ok(bytecode) => {
            let text = xcl_debug::decompile::decompile(&bytecode);
            format!("{{\"ok\":true,\"text\":{}}}", json_string(&text))
        }
        Err(errors) => format!("{{\"ok\":false,\"errors\":{}}}", errors_json(&errors)),
    }
}

/// Compiles `source` and returns the program image the byte simulator needs.
///
/// Every instruction carries its packed 64-bit word and the fields unpacked
/// from it, so the page can show both the byte layout and the decoded operands.
/// Branch instructions also carry their `target` ip, and `lines` maps the first
/// instruction of every top-level statement back to the source line, which is
/// what lets the trace player highlight the editor.
///
/// The image is compiled without the optimizing passes on purpose: those passes
/// rewrite offsets, so an unoptimized image is the only one where `ip` in a
/// trace still means the same thing as `ip` in this table.
#[wasm_bindgen]
pub fn program(source: &str) -> String {
    let (bytecode, ranges) = match compile_unoptimized(source) {
        Ok(compiled) => compiled,
        Err(errors) => {
            return format!("{{\"ok\":false,\"errors\":{}}}", errors_json(&errors));
        }
    };

    let mut out = String::with_capacity(4 * 1024);
    out.push_str("{\"ok\":true,\"instructionCount\":");
    out.push_str(&bytecode.instructions.len().to_string());
    out.push_str(",\"wideOperandCount\":");
    out.push_str(&bytecode.wide_operands.len().to_string());

    push_string_array(
        &mut out,
        "strings",
        bytecode.string_table.iter().map(|s| &**s),
    );
    push_string_array(
        &mut out,
        "locals",
        bytecode.local_names.iter().map(|s| &**s),
    );
    push_string_array(
        &mut out,
        "globals",
        bytecode.global_names.iter().map(|s| &**s),
    );
    push_string_array(
        &mut out,
        "functions",
        bytecode.user_functions.iter().map(|(name, _)| &**name),
    );

    out.push_str(",\"lines\":[");
    for (index, (ip, line)) in ranges.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('[');
        out.push_str(&ip.to_string());
        out.push(',');
        out.push_str(&line.to_string());
        out.push(']');
    }
    out.push(']');

    out.push_str(",\"code\":[");
    for (ip, instr) in bytecode.instructions.iter().enumerate() {
        if ip > 0 {
            out.push(',');
        }
        let op = instr.op();
        let (kind, name) = classify(op);
        out.push_str("{\"ip\":");
        out.push_str(&ip.to_string());
        out.push_str(",\"word\":\"0x");
        out.push_str(&format!("{:016x}", instr.0));
        out.push_str("\",\"op\":");
        out.push_str(&op.to_string());
        out.push_str(",\"name\":");
        match name {
            Some(name) => out.push_str(&json_string(name)),
            None => out.push_str("null"),
        }
        out.push_str(",\"kind\":");
        out.push_str(&json_string(kind));
        out.push_str(",\"a\":");
        out.push_str(&instr.a().to_string());
        out.push_str(",\"b\":");
        out.push_str(&instr.b().to_string());
        out.push_str(",\"c\":");
        out.push_str(&instr.c().to_string());

        // The graph view needs the edge targets, and only the VM knows which
        // operand holds one.
        let branch = xcl_vm::runtime::vm::ops::meta::branch_kind(op);
        if branch != 0 {
            let target = if branch == 1 {
                Some(instr.a() as usize)
            } else {
                bytecode
                    .wide_operands
                    .get(instr.c() as usize)
                    .map(|value| *value as usize)
            };
            out.push_str(",\"branch\":");
            out.push_str(&branch.to_string());
            if let Some(target) = target.filter(|target| *target < bytecode.instructions.len()) {
                out.push_str(",\"target\":");
                out.push_str(&target.to_string());
            }
        }

        // Resolve the operands a reader would otherwise have to look up.
        if takes_string_operand(op)
            && let Some(text) = bytecode.string_table.get(instr.a() as usize)
        {
            out.push_str(",\"string\":");
            out.push_str(&json_string(text));
        }
        if takes_slot_operand(op) {
            if let Some(name) = bytecode.local_names.get(instr.a() as usize) {
                out.push_str(",\"local\":");
                out.push_str(&json_string(name));
            } else if let Some(name) = bytecode.global_names.get(instr.a() as usize) {
                out.push_str(",\"global\":");
                out.push_str(&json_string(name));
            }
        }
        out.push('}');
    }
    out.push_str("]}");
    out
}

/// Executes `source` on the core VM and returns the final value and anything
/// the program printed.
///
/// ```json
/// {"ok":true,"value":"3","output":""}
/// {"ok":false,"value":null,"output":"","errors":[...]}
/// ```
///
/// The plugin layer is not linked (see the crate docs), so this crate registers
/// a four-function prelude — `print`, `println`, `len`, `str` — instead. Their
/// output is captured rather than written to the console.
#[wasm_bindgen]
pub fn run(source: &str) -> String {
    OUTPUT.with(|buffer| buffer.borrow_mut().clear());

    let mut vm = xcl_vm::VM::new();
    vm.register_natives(&PRELUDE);

    match xcl_vm::evaluate_recover(FILE, source, &mut vm) {
        Ok(value) => {
            let output = OUTPUT.with(|buffer| buffer.borrow().clone());
            format!(
                "{{\"ok\":true,\"value\":{},\"output\":{}}}",
                json_string(&value.as_string()),
                json_string(&output)
            )
        }
        Err(errors) => format!(
            "{{\"ok\":false,\"value\":null,\"output\":\"\",\"errors\":{}}}",
            errors_json(&errors)
        ),
    }
}

// ── Prelude ──
//
// `xcl-plugins` cannot be linked on wasm (`linkme` has no wasm support), so the
// functions the examples need are registered here by hand. They are ordinary
// `NativeKind::Pure` entries: the same shape a plugin produces.

static PRELUDE: [NativeEntry; 3] = [
    native("print", prelude_print),
    native("println", prelude_println),
    native("len", prelude_len),
];

const fn native(method: &'static str, f: fn(&[Value]) -> Value) -> NativeEntry {
    NativeEntry {
        domain: None,
        impl_type: None,
        override_method: None,
        method,
        kind: NativeKind::Pure,
        ptr: RawPtr(f as *const ()),
        args: None,
        ret: None,
        named: &[],
        summary: None,
        arg_desc: &[],
        auto_import: true,
        caps: CapSet::EMPTY,
    }
}

fn prelude_print(args: &[Value]) -> Value {
    emit(args, false);
    Value::nil()
}

fn prelude_println(args: &[Value]) -> Value {
    emit(args, true);
    Value::nil()
}

fn prelude_len(args: &[Value]) -> Value {
    let value = args.first().copied().unwrap_or_else(Value::nil);
    if let Some(text) = value.as_str() {
        return Value::from_i48(text.chars().count() as i64);
    }
    if let Some(items) = value.as_array() {
        return Value::from_i48(items.len() as i64);
    }
    if let Some(map) = value.as_map() {
        return Value::from_i48(map.len() as i64);
    }
    Value::from_i48(0)
}

/// Appends the arguments to the captured output.
///
/// A trailing map carrying only style keys (`fg_color`, `bg_color`) is the
/// named-argument pack, not a value, so it is not printed.
fn emit(args: &[Value], newline: bool) {
    let mut count = args.len();
    if let Some(map) = args.last().and_then(|value| value.as_map())
        && (map.contains_key("fg_color") || map.contains_key("bg_color"))
    {
        count -= 1;
    }

    OUTPUT.with(|buffer| {
        let mut buffer = buffer.borrow_mut();
        if buffer.len() >= OUTPUT_CAP {
            return;
        }
        for (index, value) in args[..count].iter().enumerate() {
            if index > 0 {
                buffer.push(' ');
            }
            write_value(&mut buffer, *value);
        }
        if newline {
            buffer.push('\n');
        }
    });
}

fn write_value(out: &mut String, value: Value) {
    if value.is_nil() {
        out.push_str("none");
    } else if let Some(text) = value.as_str() {
        out.push_str(text);
    } else if value.is_int() {
        out.push_str(&value.as_i48().to_string());
    } else if value.is_bool() {
        out.push_str(if value.as_bool() { "true" } else { "false" });
    } else {
        out.push_str(&value.as_string());
    }
}

/// Lexes, parses, and compiles with the optimizing passes, or returns every
/// syntax error. `run` uses this path, so the Output panel sees exactly what a
/// normal VM execution produces.
fn compile(source: &str) -> Result<Bytecode, Vec<ParseError>> {
    let tokens = xcl_vm::lexer::lex(source, FILE);
    let parsed = xcl_vm::parse_recover(&tokens, source);
    if !parsed.errors.is_empty() {
        return Err(parsed.errors);
    }
    Ok(xcl_vm::bytecode::compile(&parsed.ast))
}

/// A compiled image plus `(first_ip, line)` for every top-level statement.
type TraceImage = (Bytecode, Vec<(u32, u32)>);

/// Lexes, parses, and compiles without the optimizing passes, returning the
/// bytecode plus the statement-to-line map. `program` and `trace` share this
/// path so their instruction offsets agree.
fn compile_unoptimized(source: &str) -> Result<TraceImage, Vec<ParseError>> {
    let tokens = xcl_vm::lexer::lex(source, FILE);
    let parsed = xcl_vm::parse_recover(&tokens, source);
    if !parsed.errors.is_empty() {
        return Err(parsed.errors);
    }
    Ok(xcl_vm::bytecode::compile_unoptimized(&parsed.ast))
}

/// `,"name":["a","b"]`
fn push_string_array<'a>(out: &mut String, key: &str, items: impl Iterator<Item = &'a str>) {
    out.push_str(",\"");
    out.push_str(key);
    out.push_str("\":[");
    for (i, item) in items.enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_string(item));
    }
    out.push(']');
}

fn errors_json(errors: &[ParseError]) -> String {
    let mut out = String::from("[");
    for (i, error) in errors.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"code\":");
        out.push_str(&json_string(error.code));
        out.push_str(",\"message\":");
        out.push_str(&json_string(&error.message));
        out.push_str(",\"line\":");
        out.push_str(&error.span.line.to_string());
        out.push_str(",\"col\":");
        out.push_str(&error.span.col.to_string());
        out.push_str(",\"help\":");
        match &error.help {
            Some(help) => out.push_str(&json_string(help)),
            None => out.push_str("null"),
        }
        out.push('}');
    }
    out.push(']');
    out
}

/// Upper bound on the number of steps sent to the page.
const TRACE_STEPS: usize = 5000;

/// Runs `source` and returns the executed instruction stream with the result.
///
/// `trace` entries are `[ip, depth, opcode]`. Depth 0 is the root frame; deeper
/// entries come from a function call and refer to that function's own
/// instructions, which is why the page shows them as a call rather than a
/// Program row. `steps` is the true count; `truncated` says the array stopped
/// at [`TRACE_STEPS`].
#[wasm_bindgen]
pub fn trace(source: &str) -> String {
    OUTPUT.with(|buffer| buffer.borrow_mut().clear());

    let mut vm = VM::new();
    vm.register_natives(&PRELUDE);

    let bytecode = match compile_unoptimized(source) {
        Ok((bytecode, _)) => bytecode,
        Err(errors) => {
            return format!(
                "{{\"ok\":false,\"value\":null,\"output\":\"\",\"steps\":0,\"truncated\":false,\"errors\":{}}}",
                errors_json(&errors)
            );
        }
    };

    let value = vm.run(&bytecode);

    let output = OUTPUT.with(|buffer| buffer.borrow().clone());
    let all = vm.trace();

    let mut steps = String::with_capacity(all.len().min(TRACE_STEPS) * 14);
    steps.push('[');
    for (index, (ip, depth, op)) in all.iter().take(TRACE_STEPS).enumerate() {
        if index > 0 {
            steps.push(',');
        }
        steps.push('[');
        steps.push_str(&ip.to_string());
        steps.push(',');
        steps.push_str(&depth.to_string());
        steps.push(',');
        steps.push_str(&op.to_string());
        steps.push(']');
    }
    steps.push(']');

    format!(
        "{{\"ok\":true,\"value\":{},\"output\":{},\"steps\":{},\"truncated\":{},\"trace\":{}}}",
        json_string(&value.as_string()),
        json_string(&output),
        all.len(),
        all.len() > TRACE_STEPS,
        steps
    )
}

// ── Example plugin ──
//
// A playground-sized plugin: two instructions in the hot module space. Real
// plugins declare these with `#[opcode]` in `xcl-plugins`; the playground links
// none of that, so they are registered by hand to show what a claimed hot slot
// looks like and to prove hot dispatch works.

fn op_demo_double(vm: &mut VM, _top: usize, _instr: Instr) -> OpcodeResult {
    let value = vm.stack.pop().unwrap_or_else(Value::nil);
    vm.push_value(Value::from_i48(value.as_i48() * 2));
    OpcodeResult::Next
}

fn op_demo_negate(vm: &mut VM, _top: usize, _instr: Instr) -> OpcodeResult {
    let value = vm.stack.pop().unwrap_or_else(Value::nil);
    vm.push_value(Value::from_i48(-value.as_i48()));
    OpcodeResult::Next
}

static EXAMPLE_PLUGIN: [OpcodeEntry; 2] = [
    OpcodeEntry {
        code: 200,
        name: "OP_DEMO_DOUBLE",
        handler: op_demo_double,
    },
    OpcodeEntry {
        code: 201,
        name: "OP_DEMO_NEGATE",
        handler: op_demo_negate,
    },
];

fn plugin_name(code: u8) -> Option<&'static str> {
    EXAMPLE_PLUGIN
        .iter()
        .find(|entry| entry.code == code)
        .map(|entry| entry.name)
}

/// Runs a hand-built program that uses the example plugin's hot opcodes.
///
/// `21` → double → negate → halt, so the result is `-42`. It exists so the
/// playground can show module-space instructions in the Program tab, which
/// compiled XCL never emits.
#[wasm_bindgen]
pub fn plugin_demo() -> String {
    let instructions = vec![
        Instr::a1(xcl_vm::bytecode::OP_PUSH_INT, 21),
        Instr::none(200),
        Instr::none(201),
        Instr::none(xcl_vm::bytecode::OP_HALT),
    ];

    let bytecode = Bytecode {
        instructions: Arc::from(instructions.into_boxed_slice()),
        wide_operands: Arc::from(Vec::<i64>::new()),
        string_table: Arc::from(Vec::<Box<str>>::new()),
        string_values: Arc::from(Vec::<Value>::new()),
        local_names: Arc::from(Vec::<Box<str>>::new()),
        global_names: Arc::from(Vec::<Box<str>>::new()),
        user_functions: Arc::from(Vec::<(Box<str>, Arc<FuncValue>)>::new()),
        closures: Arc::from(Vec::<FuncValue>::new()),
    };

    let mut vm = VM::new();
    vm.register_opcodes(&EXAMPLE_PLUGIN);
    let value = vm.run(&bytecode);

    let mut code = String::from("[");
    for (ip, instr) in bytecode.instructions.iter().enumerate() {
        if ip > 0 {
            code.push(',');
        }
        let (kind, name) = classify(instr.op());
        code.push_str("{\"ip\":");
        code.push_str(&ip.to_string());
        code.push_str(",\"word\":\"0x");
        code.push_str(&format!("{:016x}", instr.0));
        code.push_str("\",\"op\":");
        code.push_str(&instr.op().to_string());
        code.push_str(",\"name\":");
        match name {
            Some(name) => code.push_str(&json_string(name)),
            None => code.push_str("null"),
        }
        code.push_str(",\"kind\":");
        code.push_str(&json_string(kind));
        code.push_str(",\"a\":");
        code.push_str(&instr.a().to_string());
        code.push_str(",\"b\":");
        code.push_str(&instr.b().to_string());
        code.push_str(",\"c\":");
        code.push_str(&instr.c().to_string());
        code.push('}');
    }
    code.push(']');

    format!(
        "{{\"ok\":true,\"value\":{},\"code\":{}}}",
        json_string(&value.as_string()),
        code
    )
}

// ── Operand conventions ──
//
// The compiler uses operand `a` for different things per opcode. These two
// predicates let the simulator label an operand instead of only numbering it.

/// Opcodes whose `a` is a string-table index.
fn takes_string_operand(op: u8) -> bool {
    use xcl_vm::bytecode::*;
    matches!(op, OP_PUSH_STR | OP_GET_FIELD | OP_TYPE_TEST | OP_TYPE_CAST)
}

/// Opcodes whose `a` is a local or global slot index.
fn takes_slot_operand(op: u8) -> bool {
    use xcl_vm::bytecode::*;
    matches!(
        op,
        OP_LOAD_LOCAL
            | OP_STORE_LOCAL
            | OP_LOAD_GLOBAL
            | OP_STORE_GLOBAL
            | OP_LOAD_LOCAL_LOAD_LOCAL
            | OP_DEC_LOCAL
            | OP_INC_LOCAL
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcode_map_covers_the_whole_byte() {
        let map = opcode_map();
        assert_eq!(map.matches("\"hex\":").count(), 256);
        assert!(map.contains("\"opExt\":127"));
        assert!(map.contains("\"moduleCount\":128"));
        // Both sides of the split are present and distinct.
        assert!(map.contains("\"kind\":\"core\""));
        assert!(map.contains("\"kind\":\"hot\""));
    }

    #[test]
    fn program_exposes_packed_instructions() {
        let json = program("1 + 2");
        assert!(json.starts_with("{\"ok\":true"), "{json}");
        assert!(json.contains("\"word\":\"0x"), "{json}");
        assert!(json.contains("\"instructionCount\":"), "{json}");
    }

    #[test]
    fn program_reports_syntax_errors_instead_of_panicking() {
        let json = program("if $x >");
        assert!(json.starts_with("{\"ok\":false"), "{json}");
        assert!(json.contains("\"errors\":["), "{json}");
    }

    #[test]
    fn diagnose_returns_the_lint_stream() {
        let json = diagnose("$unused = 1");
        assert!(json.contains("\"code\":\"W0001\""), "{json}");
        assert!(diagnose("print 1").is_empty());
    }

    #[test]
    fn decompile_returns_a_listing_or_errors() {
        let json = decompile("1 + 2");
        assert!(json.starts_with("{\"ok\":true"), "{json}");
        assert!(json.contains("OP_"), "{json}");

        let broken = decompile(")");
        assert!(broken.starts_with("{\"ok\":false"), "{broken}");
    }

    #[test]
    fn run_evaluates_core_programs() {
        let json = run("1 + 2");
        assert_eq!(json, "{\"ok\":true,\"value\":\"3\",\"output\":\"\"}");
    }

    #[test]
    fn run_captures_printed_output() {
        let json = run("print \"hello\"\nprintln 42");
        assert!(json.contains("\"output\":\"hello42\\n\""), "{json}");
    }

    #[test]
    fn prelude_len_works() {
        let json = run("println (len \"abcd\")");
        assert!(json.contains("\"output\":\"4\\n\""), "{json}");
    }

    #[test]
    fn output_does_not_leak_between_runs() {
        run("print \"first\"");
        let json = run("1 + 1");
        assert!(json.contains("\"output\":\"\""), "{json}");
    }

    #[test]
    fn plugin_demo_runs_hot_opcodes() {
        let json = plugin_demo();
        assert!(json.starts_with("{\"ok\":true"), "{json}");
        assert!(json.contains("\"value\":\"-42\""), "{json}");
        assert!(json.contains("OP_DEMO_DOUBLE"), "{json}");
        assert!(json.contains("\"kind\":\"hot\""), "{json}");
    }

    #[test]
    fn opcode_map_names_the_plugin_slots() {
        let map = opcode_map();
        assert!(map.contains("\"plugin\":{\"name\":\"example\""), "{map}");
        assert!(map.contains("OP_DEMO_NEGATE"), "{map}");
        assert!(map.contains("\"code\":200"), "{map}");
    }

    #[test]
    fn trace_records_executed_instructions() {
        let json = trace("1 + 2");
        assert!(json.starts_with("{\"ok\":true"), "{json}");
        assert!(json.contains("\"steps\":"), "{json}");
        assert!(json.contains("\"trace\":[[0,0,"), "{json}");
    }

    #[test]
    fn trace_reports_syntax_errors() {
        let json = trace(")");
        assert!(json.starts_with("{\"ok\":false"), "{json}");
    }

    #[test]
    fn run_reports_a_syntax_error() {
        let json = run(")");
        assert!(json.starts_with("{\"ok\":false"), "{json}");
    }

    #[test]
    fn program_maps_statements_to_lines() {
        let json = program("$a = 1\n$b = 2\nprint $a + $b");
        assert!(json.contains("\"lines\":[[0,1],[2,2],[4,3]]"), "{json}");
    }

    #[test]
    fn program_carries_branch_targets() {
        // A `while` loop compiles to a backward jump, so at least one row must
        // expose the target the graph view draws an edge to.
        let json = program("$i = 0\nwhile $i < 3 { $i = $i + 1 }");
        assert!(json.contains("\"branch\":"), "{json}");
        assert!(json.contains("\"target\":"), "{json}");
    }

    #[test]
    fn trace_and_run_agree() {
        // The trace runs the unoptimized image while `run` uses the optimized
        // one. Semantics must not depend on which passes ran.
        for source in [
            "1 + 2",
            "$t = 0\nfor $i in 0..10 { $t = $t + $i }\n$t",
            "sum($n) { $a = 0\n$i = 1\nwhile $i <= $n { $a = $a + $i\n$i = $i + 1 }\nreturn $a }\nsum 10",
            "print \"a\"\n1",
        ] {
            let run_json = run(source);
            let trace_json = trace(source);
            let value = run_json
                .split("\"value\":\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .unwrap_or("<missing>");
            assert!(
                trace_json.contains(&format!("\"value\":\"{value}\"")),
                "{source}\nrun:   {run_json}\ntrace: {trace_json}"
            );
        }
    }
}
