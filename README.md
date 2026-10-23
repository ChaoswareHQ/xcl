# XCL

XCL is a scripting language built on a fast, register-based virtual machine written in Rust. It uses NaN-boxed values, a bump arena for temporaries, a standard library organized into domains, and a plugin system that can register native functions, types, and even **opcodes** at compile time. An optional Cranelift JIT compiles hot loops.

```xcl
# name: hello
# version: 1.0

greet($name) {
    print fg_color: "BrightCyan" "Hello, " + $name + "!"
}

greet "world"
```

## Quick Start

```bash
# Build the launcher and the loadable interpreter library
cargo build

# Run a script file
./target/debug/xcl script.xcl

# Evaluate an expression inline
./target/debug/xcl -e 'print "Hello, world!"'

# Inspect tokens and the AST
cargo run -p xcl-debug --bin xcl-debug -- --tree script.xcl

# Disassemble bytecode
cargo run -p xcl-debug --bin xcl-bytecode -- disassemble script.xcl
```

## Language Walkthrough

### Values and Types

XCL uses **NaN-boxing** to represent every value in a single 64-bit word. Integers, floats, booleans, and `none` need no heap allocation.

| Type | Description | Examples |
|---|---|---|
| `int` | 48-bit signed integer | `42`, `0xFF`, `0b1010`, `0o755`, `1_000_000` |
| `float` | IEEE-754 double | `3.14159`, `0.5` |
| `bool` | `true` / `false` | `true`, `false` |
| `str` | UTF-8 text | `"hello"`, `r"raw\path"`, `"""multiline"""` |
| `none` | Absence of a value | `none` |
| `array` | Ordered list | `[1, 2, 3]` |
| Map | Key-value store | `{ name: "Alice", age: 30 }` |
| Range | Numeric interval | `0..10` |
| Function | Callable value | `add($a, $b) { $a + $b }` |

Integer arithmetic that exceeds the 48-bit range automatically continues as a float.

```xcl
$x = 42
$pi = 3.14159
$flag = true
$empty = none
```

### Variables

Variables are prefixed with `$` and assigned with `=`:

```xcl
$name = "Alice"
$count = 10
$count += 5        # $count = $count + 5
```

Supported compound assignments: `+=`, `-=`, `*=`, `/=`, `**=`, `|=`, `^=`, `??=`.

Type annotations use `:` between the name and the value:

```xcl
$count: i32 = 42
$price: f64 = 19.99
$name: str = "Alice"
```

### Special Variables

Inside a function, `$1`…`$N`, `$#`, `$@`, and `$*` refer to that function's arguments. `$0` is the script name. `$self` is the receiver of a method call; `$super` is the parent record of `$self` (or `none`).

```xcl
show() {
    print $#        # number of arguments
    print $@        # arguments as an array
}
show "a" "b"          # 2, [a, b]
```

Special variables are read-only. The shell-style variables `$$`, `$?`, `$!`, and `$-` are deliberately unsupported and produce a clear error.

### Operators

**Arithmetic and comparison:**

```xcl
$a = 10 + 3     $b = 10 - 3     $c = 10 * 3
$d = 10 / 3     $e = 10 % 3     $f = 2 ** 10
$x == $y        $x != $y        $x < $y         $x >= $y
```

**Logical:**

```xcl
$flag1 and $flag2
$flag1 or $flag2
not $flag
$flag1 && $flag2    # same as `and`
$flag1 || $flag2    # same as `or`
```

**Bitwise:** `|`, `^`, `&`, `~`, `<<`, `>>`.

```xcl
$mask = 0xF0 | 0x0F
$flip = $flags ^ 1
$low  = $value & 0xFF
$not  = ~$value
$half = $value >> 1
```

**Null coalescing and safe navigation:**

```xcl
$name = $input ?? "default"
$config ??= load_defaults()
$city = $user?.address?.city      # none instead of a crash
```

**Conditional expression** (no `?:` operator):

```xcl
$label = "WEEKEND" if $day > 5 else "Workday"
```

**Pipeline** — the left value becomes the first argument of the named stage:

```xcl
$sum = 10 |> add 5      # `add` is defined in the Functions section
$upper = "hi" |> util.print
```

**Range, membership, tests, casts:**

```xcl
for $i in 0..10 { ... }
if $key in $map { ... }
$value as f64
```

Operator precedence, lowest to highest: conditional (`x if c else y`), `|>`, `??`, `or`/`||`, `and`/`&&`, `==`/`!=`, `|`, `^`, `&`, comparisons and `in`/`is`/`as`, `..`, `+`/`-`, `<<`/`>>`, `*`/`/`/`%`, `**`, unary, postfix, primary.

> **Note:** `|` is bitwise OR, so a pipe lambda passed as an argument must be parenthesized: `f (|$x| $x + 1)`.

### Strings

```xcl
$normal = "Hello\nWorld"          # escapes processed
$raw = r"C:\path\to\file"         # no escape processing
$multi = """Line 1
Line 2"""                         # multiline
```

Concatenate with `+`. There is no interpolation — `"$name"` is literal:

```xcl
$greeting = "Hello, " + $name + "!"
```

### Comments

```xcl
# A comment
$x = 42  # inline comment
```

A **leading** block of `# key: value` lines is a comment header. Runtime
metadata access is not part of this release.

```xcl
# name: deploy
# version: 2.1.0
# author: ada
```

### Control Flow

```xcl
if $x > 10 {
    print "big"
} else if $x > 5 {
    print "medium"
} else {
    print "small"
}

for $i in 0..10 {
    if $i == 5 { break }
    if $i % 2 == 0 { continue }
    print $i
}

while $x > 0 {
    $x = $x - 1
}
```

`break` leaves and `continue` restarts the innermost `for` or `while` loop. Both are
statements, so they work inside an `if` branch, and neither escapes a function or
lambda boundary.

### Functions

Declare a function with its name, parameters, and body — no keyword:

```xcl
add($a, $b) {
    $a + $b
}

greet($name) {
    return "Hello, " + $name
}
```

Parameters and return types may be annotated:

```xcl
calculate($a: i32, $b: f64) -> f64 {
    return $a + $b
}
```

Calls use **space-separated arguments**. Parentheses are not call syntax — they delimit parameters in a definition:

```xcl
$result = add 3 4

# ILLEGAL — `add(...)` is not a call
# add(3, 4)
```

A *spaced* parenthesis is a grouped expression passed as one argument:

```xcl
add (1 + 2) 3          # 6
apply (|$x| $x + 1) 41
```

`name(...)` followed by `{` or `->` is a function definition, so definitions are unaffected.

A zero-argument function is called by writing its name at a statement boundary, or in a condition:

```xcl
status() {
    return "ok"
}

status
if status { print "ready" }
```

Function values are called with `$callee args`, and a zero-argument value call is a bare `$f` statement:

```xcl
$double = |$x| $x * 2
$result = $double 21          # 42

$greet = { print "hi" }
$greet                        # calls it (zero arguments)

apply($f, $v) {
    return $f $v
}
apply (|$x| $x + 1) 41        # 42
```

Writing `$x` for a non-function value is just a value read, so a trailing `$x` still yields the script's result.

**Lambdas** use pipes:

```xcl
$square = |$x| $x * $x
$add = |$a, $b| $a + $b
$block = { |$x, $y| $x + $y }   # block lambda
```

Lambdas capture values from the enclosing scope **by value**: the value is fixed when the lambda is created, so rebinding the outer variable later does not affect the closure. Parameters and names assigned in the lambda's body shadow the captured names, and all parameters must be declared explicitly.

```xcl
make_adder($n) { return |$x| $x + $n }
$add10 = make_adder 10
$add10 5                       # 15
```

### Top-Level Variables in Functions

Top-level variables are visible inside functions and lambdas:

```xcl
$limit = 10
below($n) { return $n < $limit }
below 5                        # true
```

They are read-only from a function body. Assigning the same name inside a function creates a local that shadows the top-level variable for the whole body:

```xcl
$x = 5
set($v) { $x = $v; return $x }
$local = set 9                 # 9 — the function's own $x
$x                             # 5 — the top-level variable
```

### Named Arguments

Named arguments use `name: value` and may be mixed with positional ones:

```xcl
print bg_color: "red", fg_color: "black" "hello"
```

At the call site all named arguments are packed into **one trailing map**, so a native receives them as its last positional argument (relevant when writing plugins). Because `:` introduces a name, `=` is unambiguously assignment.

An explicit `,` continues the argument list, so it may span lines:

```xcl
print bg_color: "red",
        fg_color: "black" "hello"
```

### Brace Expansion

A brace group with no `key:` pairs after a call expands into one call per item, separated by `;` or a newline:

```xcl
print { "this"; "here" }        # two calls
```

A `,` after an item introduces that item's own named arguments, which override the call-level ones:

```xcl
print bg_color: "red", fg_color: "black" {
    "hello", fg_color: "red"    # black text on red
    "hi"                        # red text on red
}
```

The group is an expansion only when every argument before it is named (or there are none) and the braces contain no `key: value` pairs. Otherwise the braces keep their meaning: a map literal or a block-lambda.

### Arrays

```xcl
$scores = [3, 2, 5]
print $scores[0]      # 3
$scores = $scores + [7]
```

Negative indices are not supported.

### Ranges

`start..end` is a value in expression position and a fused numeric loop in a `for` header:

```xcl
for $i in 0..4 { print $i }   # 0 1 2 3

$r = 0..4                     # a value: {start: 0, end: 4}
for $i in $r { print $i }     # 0 1 2 3

bounds($n) { return 0..$n }
for $i in (bounds 3) { print $i }
```

A range is a map with `start` (inclusive) and `end` (exclusive) fields.

### Maps

```xcl
$person = { name: "Alice", age: 30 }
print $person.name      # "Alice"
print $person["age"]    # 30
```

Fields and elements are writable. A write to a field that does not exist yet
inserts it:

```xcl
$person.age = 31
$person.city = "Oslo"   # new field
$person["age"] += 1     # compound writes work too

$scores = [10, 20, 30]
$scores[1] = 99
$scores[-1] = 0         # negative offsets count from the end
$scores[3] = 5          # one past the end appends
```

### Custom Types

**Record types** list named, typed fields:

```xcl
Point = {
    x: f64
    y: f64
}

$p = Point { x: 1.0, y: 2.0 }
print $p.x
```

A plain map literal still works when no type is involved: `$p = { x: 1.0, y: 2.0 }`.

**Sum types** (tagged unions) use `|`:

```xcl
Direction = Left | Right | Up | Down
Message = Text { content: str } | Image { url: str } | Empty
```

Sum-type values carry a `tag` field, which is what `is` tests:

```xcl
$msg = Message.Text { content: "hi" }
if $msg is Text { print $msg.content }
```

The same value can be written as a map literal: `{ tag: "Text", content: "hi" }`.

**Traits** are types containing only method signatures, implemented with `is`:

```xcl
Drawable = {
    draw() { }
}

Button is Drawable {
    draw() { print "drawing button" }
}
```

Methods receive their receiver as the first argument, so `$self` names it inside
the body. A function can also be reached as a method on any value: `$e.tick 1`
passes `$e` first. A field with the same name takes precedence over the method.

```xcl
tick($v, $d) {
    $self.hp = $self.hp - $d
    return $self.hp
}

$e = { hp: 100 }
print $e.tick 3         # 97, with $self bound to $e
print $e.hp             # 97
```

### Type Casting

```xcl
$float_value = $raw as f64
$int_value = $raw as i32
```

`is` tests the `tag` of a map (how sum-type values are represented); it does not test primitive types.

### Imports

A top-level line binds a definition from another file:

```xcl
hello = test.hello

hello
```

This loads `test.xcl` next to the importing file and binds its top-level
function `hello`. The part after `=` is a dotted path: every segment but the
last names the module file, and the last is the symbol. Nested paths such as
`add = lib.math.add` load `lib/math.xcl`.

The local name may differ from the symbol: `greeting = test.hello` binds the
function as `greeting`. Types keep their declared name (`Point = shape.Point`).
Imported files may import others; each module is included once, and a circular
import is reported as an error. Import lines must be top-level statements on
their own line.

### Domains

The standard library is organized into **domains** (namespaces). `xcl-std`
ships exactly one domain, `util`, with two entry points: `print` (output) and
`input`. Other domains (`math`, `io`, `fs`, `net`, `encoding`) are not part of
this release.

```xcl
print "hello"                 # util.print
$name = input "name: "        # util.input
```

### Auto-Import

Methods marked `auto_import` are also available as bare identifiers. Today that
is the `util` print/input family:

```xcl
print "Hello"                 # util.print
$line = input "name: "        # util.input
$n = util.write "raw"         # low-level output, returns bytes written
util.flush                    # flush stdout
```

Every other method must be qualified: `util.write`, `util.readline`.

## Standard Library

All of this lives in `xcl-std`, under `std/`. `xcl-vm` itself contains only
primitives and the language axioms (`$self`, `$super`, special variables).

```text
xcl-std/                 one crate
└── std/                     folder chain, not a crate
    └── util/                the library (crate root: lib.rs)
        ├── lib.rs           `#[xcl_plugin]` marker, host API
        ├── print.rs         print, write, flush
        └── input.rs         input, readline
```

| Domain | Purpose | Auto-import |
|---|---|---|
| `util` | Fast console output and standard input | `print`, `input`, `readline`, `write`, `flush` |

`std` is the plugin that owns `util`, so a call may also be written with the
qualified path — all three spellings are the same function:

```xcl
print "hi"              # auto-import
util.print "hi"         # flat domain
std.util.print "hi"     # plugin-qualified path
```

The VM records the mapping (`VM::plugins()`, `VM::plugin_of("util")`), so a host
can ask which plugin provides a domain without importing the plugin crate.

### Output

`print` is variadic and writes to one locked stdout handle. Strings, ints,
booleans and nil do not allocate an intermediate `String`.

```xcl
print "plain"
print "two" "values" 1
print ""                # no trailing newline; use util.write "\n" for one
```

`util.write` writes raw text and returns the number of bytes written;
`util.flush` flushes buffered stdout.

### Input

```xcl
$name = input "name: "       # prompt, one line
$line = util.readline         # one line, no prompt
```

`input` and `readline` return `none` at end of input.

## Architecture

```
Source → Lexer → Parser → Compiler → Bytecode → VM → Output
```

1. **Lexing** — `logos`-based DFA lexer; punctuation and keywords are zero-allocation.
2. **Parsing** — recursive descent plus a Pratt expression parser.
3. **Compilation** — the AST becomes 64-bit instructions, then peephole and dataflow passes run (constant folding, dead-store elimination, LICM, superinstruction fusion).
4. **Execution** — a register-file VM with a shared operand stack and `LocalSlice` frames.

### Loadable Runtime

`xcl` is a thin launcher. The interpreter is built as a shared library and
loaded at run time:

- Windows — `xcl_rt.dll`
- Linux — `libxcl_rt.so`
- macOS — `libxcl_rt.dylib`

The library contains the VM, the standard plugins, the import loader, the
linter/disassembler, and mimalloc. The launcher looks for it in `XCL_RUNTIME`,
then next to `xcl`, then in that directory's `deps` folder. The ABI is versioned
with `xcl_rt_abi_version`; no allocated value crosses the boundary — the
host passes UTF-8 paths/source and receives process exit codes.

| Export | Purpose |
|---|---|
| `xcl_rt_abi_version` | ABI version, checked by the launcher |
| `xcl_rt_abi_list` | Print the exported ABI |
| `xcl_rt_new` / `xcl_rt_free` | Runtime context |
| `xcl_rt_set_quiet` / `set_format` | Output configuration |
| `xcl_rt_add_level` | `-A/-W/-D/-F` lint overrides |
| `xcl_rt_set_script_args` | `$1`, `$2`, ... |
| `xcl_rt_set_features` | Select compiled plugins |
| `xcl_rt_list_plugins` | List compiled plugins |
| `xcl_rt_manifest` | Print the generated plugin manifest (TOML) |
| `xcl_rt_run_file` / `run_source` | Resolve imports, compile, run |
| `xcl_rt_decompile_file` / `decompile_source` | Disassemble |
| `xcl_rt_lint_file` / `lint_source` | Lint |
| `xcl_rt_list_lints` / `explain` | Rule catalogue |

A C host embeds it the same way it would embed Luau:

```c
#include <windows.h>

typedef void *(*xcl_new_fn)(void);
typedef void (*xcl_free_fn)(void *);
typedef int (*xcl_run_file_fn)(void *, const char *);

HMODULE lib = LoadLibraryA("xcl_rt.dll");
xcl_new_fn new = (xcl_new_fn)GetProcAddress(lib, "xcl_rt_new");
xcl_run_file_fn run_file =
    (xcl_run_file_fn)GetProcAddress(lib, "xcl_rt_run_file");
xcl_free_fn free = (xcl_free_fn)GetProcAddress(lib, "xcl_rt_free");

void *ctx = new();
int status = run_file(ctx, "test.xcl");
free(ctx);
/* Keep `lib` loaded: Rust thread-local destructors must stay valid. */
```

Build both pieces together with `cargo build`; `cargo build -p xcl-runtime`
builds only the library.

### Features

Optional standard-library plugins are compiled into the runtime and selected at
run time. They live in `xcl_std::FEATURE_REGISTRY`, a
`linkme::distributed_slice`, so a new plugin crate adds a plugin without
changing the runtime or the launcher.

`xcl list` shows what is compiled in, `xcl manifest` prints the generated
plugin manifest, and `xcl abi` shows the C surface:

```bash
xcl list
xcl manifest
xcl abi
```

There is one plugin today. `std` is the default and installs the `util`
domain:

| Plugin | Domains | Adds |
|---|---|---|
| `std` | `util` | `print`, `input`, `readline`, `write`, `flush` |

```bash
xcl --features std -e 'print "hi"'
xcl --no-default-features --features std -e 'print "minimal"'
xcl --no-default-features -e '1 + 2'   # core language, no plugins
xcl --all-features -e 'print "hi"'
```

A host selects plugins with `xcl_rt_set_features(ctx, csv, mode)`: mode `0` is
defaults, `1` defaults plus `csv`, `2` only `csv` (plus dependencies), and `3`
every compiled plugin. Dependencies are installed first, and a plugin's
`install` function registers only the domains it owns. A context starts with the
default plugins, so an embedding host that does nothing gets `std`.

A plugin crate declares itself once, on its domain marker type:

```rust
#[xcl_plugin(default, domain = "my_domain", summary = "my_plugin — example domain")]
pub struct MyDomain;
```

`#[xcl_plugin]` emits the registry anchors, the `PLUGIN_REGISTRY` entry, and the
`FEATURE_REGISTRY` entry, and derives `Xcl` for the marker. Functions, impls, and
types then use `#[xcl_fn]`, `#[xcl_impl]`, and `#[derive(Xcl)]` and never name a
slice. The plugin name comes from the crate (`xcl-my-plugin` → `my_plugin`) and
the domain from the type (`Util` → `util`); both can be overridden with
`name = "..."` / `domain = "..."`.

`register_domains` installs only the natives, types, traits, and plugin
descriptors whose domain is listed; `true` also includes domain-less globals.
Opcodes and impl blocks are always registered because unused opcodes are
harmless and impls may be shared by several domains.

### Memory Model

- **NaN-boxing** — every value is one 64-bit word; heap objects are pointers in the low bits.
- **Bump arena** — temporaries come from a thread-local, pooled bump arena. `run()` resets it and keeps the current chunk, so repeated runs do not grow memory.
- **Escape barrier** — values returned from `run` are copied out of the arena, so callers can inspect them safely.

### JIT

`xcl-jit` compiles hot loops to native code with Cranelift. It is optional and
must be compiled into the loadable runtime:

```bash
cargo build --release -p xcl-runtime --features jit
```

The JIT covers integer loops, comparisons, arithmetic, array indexing, and map field reads. Anything it cannot compile falls back to the interpreter, so semantics are identical.

### Opcode Registry

Opcodes are declared with the `#[opcode]` macro:

```rust
use xcl_macros::opcode;
use xcl_vm::{Instr, OpcodeResult, Value};

#[opcode(name = "OP_MY_THING", code = 200)]
fn op_my_thing(vm: &mut xcl_vm::VM, _top: usize, _instr: Instr) -> OpcodeResult {
    vm.push_value(Value::from_i48(42));
    OpcodeResult::Next
}
```

The code space is `0..=126` for core instructions, `127` for `OP_EXT` (a 32-bit module id in the operand for cold modules), and `128..=255` for hot plugin module slots. Adding a core opcode also requires registering it with every optimizer pass (see `.agents/skills/xcl-perf/SKILL.md`).

## Project Structure

| Crate | Description |
|---|---|
| `xcl-vm` | Core VM: lexer, parser, bytecode compiler, optimizer, runtime, opcodes |
| `xcl-jit` | Optional Cranelift JIT backend (`jit` feature) |
| `xcl-macros` | Procedural macros: `#[xcl_fn]`, `#[xcl_impl]`, `#[xcl_trait]`, `#[derive(Xcl)]`, `#[opcode]` |
| `xcl-std` | Standard library plugin (`std`): the `util` domain with `print` and `input` |
| `xcl-cli` | Thin launcher: loads the runtime library and forwards commands |
| `xcl-runtime` | Loadable interpreter (`cdylib`): VM, plugins, imports, diagnostics, mimalloc, C ABI |
| `xcl-debug` | `xcl-debug` (tokens/AST) and `xcl-bytecode` (disassemble/trace/profile/stats) |
| `xcl-doc` | Documentation generator that reads macro metadata and produces an HTML site |
| `xcl-analysis` | Linter: rustc-style diagnostics, rule catalogue, `--lint` JSON contract |
| `xcl-wasm` | Browser build for the playground: lint, disassemble, run, opcode map (`wasm32-unknown-unknown`) |
| `xcl-benchmark` | Benchmark corpus and cross-language harnesses: `crosslang/run.py`, `crosslang/usage.py`, the Luau/LuaJIT runners, and `scripts/score.py` |

## Writing Native Functions

Use the `#[xcl_fn]` macro; it derives the domain and method from the Rust function name (`fs_open` → `fs.open`):

```rust
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Greet a user by name.
#[xcl_fn]
pub fn io_greet(name: &str) -> Value {
    Value::from_str_boxed(format!("Hello, {}!", name))
}
```

For impl blocks, use `#[derive(Xcl)]` plus `#[xcl_impl]`; add `auto_import` to expose bare names:

```rust
use xcl_macros::{Xcl, xcl_impl};

#[derive(Xcl)]
pub struct MyDomain;

#[xcl_impl(auto_import)]
impl MyDomain {
    /// Print a message.
    pub fn print(text: String) {
        println!("{}", text);
    }
}
```

Variadics receive `&[Value]` and named arguments arrive as one trailing map, so native functions split them with `split_named_args(args, &["fg_color", "bg_color"])` (crate-internal in `print.rs`).

Registration uses `linkme` distributed slices, so it costs nothing at runtime.

### Hot Native Calls

Add `hot` to a native that is called often. The VM then gives it a slot, and the
compiler emits `OP_CALL_HOT` instead of a named call, which skips the name hash
and the global-table probe on every call:

```rust
#[xcl_fn(name = "util.print", auto_import, hot)]
pub fn print(args: &[Value]) -> Value { /* ... */ }
```

The flag is a hint, never a semantic change: a name that is shadowed or claimed
twice keeps its ordinary `OP_CALL_NAME` call, so declaring a native hot cannot
alter what a call means. Slots are numbered per VM, and `xcl_vm::evaluate`
compiles against the VM it runs, so nothing has to be wired up by hand. Bytecode
built with `xcl_vm::bytecode::compile` (no VM) stays portable and contains no hot
calls.

## Tools

### Parser Debug Tool

```bash
cargo run -p xcl-debug --bin xcl-debug -- --tokens script.xcl
cargo run -p xcl-debug --bin xcl-debug -- --tree script.xcl
cargo run -p xcl-debug --bin xcl-debug -- --tree --verbose script.xcl
```

### Bytecode Tool

```bash
cargo run -p xcl-debug --bin xcl-bytecode -- disassemble script.xcl
cargo run -p xcl-debug --bin xcl-bytecode -- trace script.xcl
cargo run -p xcl-debug --bin xcl-bytecode -- profile script.xcl
cargo run -p xcl-debug --bin xcl-bytecode -- stats script.xcl
```

### Documentation Generator

```bash
cargo run -p xcl-doc --release -- build --out docs
cargo run -p xcl-doc --release -- serve
```

The generated site includes domain pages, function references, search, and dark mode.

## CLI Usage

```
xcl [OPTIONS] [FILE]...

Options:
  -h, --help              Show help
  -v, --version           Show version
  -e, --eval <EXPR>       Evaluate the given expression
  -q, --quiet             Suppress result output
  -d, --decompile         Decompile to bytecode and print (do not run)
  --                      All following arguments are script arguments ($1, $2, ...)

Linting:
  --lint                  Analyse and report diagnostics instead of running
  --error-format <FMT>    `human` (default) or `json` (one object per line)
  --explain <CODE>        Explain a rule by code (W0001) or name (unused_binding)
  --list-lints            List every rule, its group, and its default level
  -A, --allow <TARGET>    Silence a rule, rule group, or `warnings`
  -W, --warn <TARGET>     Report a rule as a warning
  -D, --deny <TARGET>     Report a rule as an error
  -F, --forbid <TARGET>   Report a rule as an error; cannot be overridden later
      --deny-warnings     Shorthand for `-D warnings`
```

```bash
xcl script.xcl -- a b "c d"
xcl -e 'print $1' -- hello
xcl -d -e 'print 1 + 2'

# Linting
xcl --lint script.xcl                     # warnings on stderr, exit 0
xcl -D warnings --lint script.xcl         # warnings become errors, exit 1
xcl --lint --error-format=json script.xcl # NDJSON on stdout, for tools and agents
xcl -A style --lint script.xcl            # silence a whole group
xcl --explain W0001                       # what the rule means and how to fix it
```

The linter and disassembler live in `xcl-runtime` and are always available.
The launcher's `lint`/`debug` features are accepted for compatibility but no
longer change the build.

## Building and Benchmarking

```bash
# Launcher + loadable interpreter runtime
cargo build --release

# Interpreter runtime + Cranelift JIT
cargo build --release -p xcl-runtime --features jit

# Tests and lints
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check -p xcl-vm --no-default-features   # no_std core
```

Single-language benchmarks:

```bash
cargo bench -p xcl-vm --bench vm_core --bench vm_data \
    --bench vm_algorithms --bench vm_games
```

Cross-language comparison against Luau, LuaJIT, and CPython:

```bash
# Keep a JIT runtime next to a copy of the launcher, then rebuild the default
# runtime for the interpreter column.
cargo build --release -p xcl-runtime --features jit
mkdir -p target/release/jit
cp target/release/xcl target/release/jit/xcl
cp target/release/xcl_rt.dll target/release/jit/   # libxcl_rt.so on Unix
cargo build --release -p xcl-runtime
CROSSLANG_REPS=9 python crates/xcl-benchmark/crosslang/run.py
python crates/xcl-benchmark/crosslang/usage.py
```

On the reference machine the interpreter beats Luau on all seven cross-language workloads, and the JIT is within roughly 1.2x of LuaJIT while using less memory than CPython.

## Examples

See [`example/`](example/) for sample scripts, and `crates/xcl-vm/benches/` for the benchmark programs used in the comparison suite.

## Contributing

Contributions are welcome. The most impactful way to contribute is writing domain plugins — each new domain extends the standard library without touching the VM — or adding opcodes for hot patterns, provided they are registered with every optimizer pass.

## License

Licensed under either MIT or Apache-2.0.

XCL was created by [Nguyễn Quý Ngọc](https://github.com/ChaoswareHQ).
