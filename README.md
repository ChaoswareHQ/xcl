# XCL (Xcelerate)

A scripting language with a fast VM, standard library, and CLI.

## Quick Start

```bash
# Run a file
cargo run --release -- file.xcl

# Evaluate an expression
cargo run --release -- -e 'io.print "hello" 42'
```

## Project Structure

| Crate | Description |
|---|---|
| `xcl-vm` | Core VM: lexer, parser, bytecode compiler, and runtime |
| `xcl-macros` | Procedural macros: `#[xcl_fn]` for native functions, `#[xcl_type]` for type descriptors |
| `xcl-std` | Standard library: `io.*` functions, `Ok`/`Err` constructors, `print`, `format`, `input` |
| `xcl-cli` | Command-line interface |
| `xcl-docs` | Documentation generator — reads macro metadata and produces Markdown |

## Writing Native Functions

Use the `#[xcl_fn]` macro to register Rust functions as XCL builtins:

```rust
use xcl_macros::xcl_fn;
use xcl_vm::Value;

#[xcl_fn(
    name = "greet",
    domain = "std",
    args = "name: str",
    ret = "str",
    summary = "Return a greeting string.",
    arg_desc("name" = "The person's name."),
)]
pub fn greet_fn(name: &str) -> Value {
    Value::from_str_boxed(format!("Hello, {}!", name))
}
```

### `#[xcl_fn]` Attributes

| Attribute | Required | Description |
|---|---|---|
| `name` | Yes | The XCL function name |
| `domain` | No | Namespace prefix (e.g., `"io"`) — produces `io.name` |
| `args` | No | Signature string, e.g., `"path: str; mode: str"` |
| `ret` | No | Return type string, e.g., `"Result"` |
| `summary` | No | One-line description shown in generated docs |
| `arg_desc` | No | Per-argument descriptions: `arg_desc("path" = "File path")` |

### `#[xcl_type]` Attributes

| Attribute | Required | Description |
|---|---|---|
| `name` | Yes | The XCL type name |
| `domain` | No | Namespace prefix |
| `kind` | No | `"Opaque"`, `"Record"`, `"Sum"`, or `"Alias"` |
| `summary` | No | One-line description |
| `fields` | No | Struct fields: `fields { name: "str", age: "i64" }` |
| `variants` | No | Sum variants: `variants { Ok { value: "any" }, Err { msg: "str" } }` |

## Generating Documentation

```bash
cargo run -p xcl-docs --release
```

This reads all `#[xcl_fn]` and `#[xcl_type]` entries and writes `docs/README.md`.

## CLI Usage

```
xcl [OPTIONS] [FILE]

Options:
  -h, --help            Show help
  -v, --version         Show version
  -e, --eval <EXPR>     Evaluate an expression
  -q, --quiet           Suppress output
  --                    End of options
```

## License

MIT OR Apache-2.0