# xcl-wasm

The XCL compiler and tooling, built for the browser. This crate is the engine
behind the website playground: it lints, disassembles, and executes XCL, and it
exposes the whole opcode space so a page can render the bytecode map.

It compiles on the host too — outside wasm, `#[wasm_bindgen]` emits ordinary Rust
functions — which is how it is tested (`cargo test -p xcl-wasm`).

## Build

```sh
# 1. The wasm artifact
cargo build -p xcl-wasm --target wasm32-unknown-unknown --release

# 2. The JS glue (version must match the `wasm-bindgen` dependency)
cargo install wasm-bindgen-cli --version 0.2.129
wasm-bindgen --target web --out-dir <site>/wwwroot/playground \
    target/wasm32-unknown-unknown/release/xcl_wasm.wasm
```

The page then imports the generated module:

```js
import init, * as xcl from "./playground/xcl_wasm.js";
await init();
console.log(xcl.run("1 + 2"));   // {"ok":true,"value":"3"}
```

## API

Every export takes a source string (or nothing) and returns JSON, so the client
never parses prose.

| Export | Returns |
| --- | --- |
| `version()` | the crate version as a string |
| `opcode_map()` | the 256-byte opcode space, classified |
| `diagnose(source)` | NDJSON diagnostics, one object per line (the `--lint --error-format=json` contract); empty string when clean |
| `decompile(source)` | `{"ok":bool,"text":string}` — the disassembly |
| `program(source)` | the compiled image: instruction words, decoded operands, tables |
| `run(source)` | `{"ok":bool,"value":string,"output":string}` — executes and captures `print` |
| `trace(source)` | `run` plus `trace: [[ip, depth, opcode], …]` — the executed instruction stream (capped at 5000 steps) |
| `plugin_demo()` | runs hand-built bytecode through the example plugin's two hot opcodes |

### `program` shape

```json
{
  "ok": true,
  "instructionCount": 3,
  "wideOperandCount": 0,
  "strings": ["a", "b"],
  "locals": ["x"],
  "globals": ["f"],
  "functions": ["f"],
  "code": [
    {"ip":0,"word":"0x0000000004000000","op":4,"name":"OP_PUSH_INT",
     "kind":"core","a":42,"b":0,"c":0}
  ]
}
```

Instruction words are packed `op:8 | a:32 | b:16 | c:8`; the JSON carries both
the raw word and the unpacked fields. `kind` is `core`, `ext`, `growth`, `hot`,
or `unused`, and a decoded `string`/`local`/`global` is added when operand `a`
is an index the page would otherwise have to resolve itself.

### `opcode_map` shape

```json
{
  "coreFirst": 0, "coreLast": 127, "opExt": 127,
  "moduleFirst": 128, "moduleCount": 128,
  "entries": [{"code":0,"hex":"0x00","kind":"core","name":"OP_NOP","slot":null}]
}
```

`entries` always has 256 elements. A hot entry's slot is `code & 0x7f`.

## What is linked

`xcl-vm`, `xcl-analysis`, and the `xcl-debug` decompiler — **not** `xcl-std`.

The plugin layer registers itself through `linkme` distributed slices, which
`wasm32-unknown-unknown` does not support: the linker never emits the section
boundaries, so every registry reads as empty. The split is therefore explicit:

- **Available:** primitives, operators, control flow, functions, closures,
  records, arrays, maps, `type`/`super`, linting, disassembly, the opcode map,
  and a three-function prelude registered by hand:

  | Name | Behaviour |
  | --- | --- |
  | `print` | joins its arguments with a space, no newline |
  | `println` | same, with a newline |
  | `len` | characters in a string, items in an array, keys in a map |

  `output` in the `run` reply is whatever those three wrote; it is capped at
  64 KiB so a runaway loop cannot exhaust the heap.
- **Not available:** std domains (`util.*`, `math.*`, …), `fs`, `io`, `https`,
  and plugin opcodes. The 128 hot slots read as unclaimed.

The prelude is deliberately tiny. Anything larger should go through an
explicit registry in `xcl-std` rather than being reimplemented here.
