# XCL

XCL is a scripting language built on a fast, stack-based virtual machine written in Rust. It features NaN-boxed value representation, bump allocation for fast temporary objects, a rich standard library organized into domains, auto-import for convenient bare-identifier calls, and a pipeline-friendly syntax inspired by Rust and functional languages.

## Quick Start

```bash
# Run a script file
cargo run -p xcl-cli -- script.xcl

# Evaluate an expression inline
cargo run -p xcl-cli -- -e 'print "Hello, world!"'

# Debug parser issues
cargo run -p xcl-debug -- --tree script.xcl
```

## Language Walkthrough

This section walks through the XCL language from basic values to advanced features.

### Values and Types

XCL uses **NaN-boxing** to represent all values in a single 64-bit word. Primitive types — integers, floats, booleans, and `none` — require no heap allocation.

| Type | Description | Examples |
|---|---|---|
| Integer | 48-bit two's complement | `42`, `0xFF`, `0b1010`, `0o755`, `1_000_000` |
| Float | IEEE-754 double precision | `3.14159`, `0.5` |
| Boolean | True or false | `true`, `false` |
| String | UTF-8 text | `"hello"`, `r"raw\path"`, `"""multiline"""` |
| None | Absence of a value | `none` |
| Array | Ordered list | `[1, 2, 3]` |
| Map | Key-value store | `{ name: "Alice", age: 30 }` |
| Function | Callable block | `add($a, $b) { $a + $b }` |

**Integer overflow:** When integer arithmetic exceeds 48 bits, XCL automatically falls back to floating-point representation without raising an error.

```xcl
$x = 42
$pi = 3.14159
$flag = true
$empty = none
```

### Primitive Type Keywords

XCL supports Rust-like primitive type keywords for type annotations and type tests:

| Keyword | Type | Description |
|---|---|---|
| `i8` | Signed 8-bit integer | -128 to 127 |
| `i16` | Signed 16-bit integer | -32,768 to 32,767 |
| `i32` | Signed 32-bit integer | Standard integer size |
| `i64` | Signed 64-bit integer | Large signed integers |
| `u8` | Unsigned 8-bit integer | 0 to 255 |
| `u16` | Unsigned 16-bit integer | 0 to 65,535 |
| `u32` | Unsigned 32-bit integer | Standard unsigned size |
| `u64` | Unsigned 64-bit integer | Large unsigned integers |
| `f32` | 32-bit float | Single precision |
| `f64` | 64-bit float | Double precision |
| `str` | String | UTF-8 text |
| `bool` | Boolean | true/false |
| `any` | Dynamic | Any type |

```xcl
$count: i32 = 42
$price: f64 = 19.99
$name: str = "Alice"
$active: bool = true
```

### Variables

Variables are prefixed with `$` and assigned with `=`. The `:=` operator creates a new binding declaration.

```xcl
$name = "Alice"
$count = 10
$count := 20       # re-declare binding
$count += 5        # compound assignment
```

XCL supports compound assignment operators: `+=`, `-=`, `*=`, `/=`.

### Operators

**Arithmetic:**

```xcl
$a = 10 + 3    # addition
$b = 10 - 3    # subtraction
$c = 10 * 3    # multiplication
$d = 10 / 3    # division
$e = 10 % 3    # modulo
```

**Comparison:**

```xcl
$x == $y    # equal
$x != $y    # not equal
$x < $y     # less than
$x > $y     # greater than
$x <= $y    # less than or equal
$x >= $y    # greater than or equal
```

**Logical:**

```xcl
$flag1 and $flag2    # logical AND
$flag1 or $flag2     # logical OR
not $flag            # logical NOT

# C-style operators are also supported
$flag1 && $flag2
$flag1 || $flag2
```

**Null coalescing:** The `??` operator returns the right-hand value when the left-hand value is `none`.

```xcl
$name = $input ?? "default"
$config ??= loadConfig()
```

**Safe navigation:** The `?.` operator short-circuits to `none` if the left-hand value is `none`.

```xcl
$city = $user?.address?.city
```

**Pipeline:** The `|>` operator passes the left-hand value as the first argument to the right-hand function.

```xcl
"hello" |> string.uppercase |> print
```

### Strings

XCL supports three string literal forms:

```xcl
$normal = "Hello\nWorld"          # escape sequences processed
$raw = r"C:\path\to\file"         # no escape processing
$multi = """Line 1
Line 2"""                        # multiline string
```

Strings can be concatenated with `+`:

```xcl
$greeting = "Hello, " + $name + "!"
```

For formatted output, use the `format` function:

```xcl
print format("Name: {}, Age: {}" $name $age)
```

### Comments

Single-line comments start with `#`:

```xcl
# This is a comment
$x = 42  # Inline comment
```

### Control Flow

**Conditionals:**

```xcl
if $x > 10 {
    print "big"
} else if $x > 5 {
    print "medium"
} else {
    print "small"
}
```

**For loops** iterate over ranges or collections:

```xcl
for $i in 0..10 {
    print $i
}
```

**While loops** repeat while a condition is true:

```xcl
while $x > 0 {
    $x = $x - 1
}
```

Use `break` to exit a loop and `continue` to skip to the next iteration:

```xcl
for $i in 0..100 {
    if $i == 42 { break }
    if $i % 2 == 0 { continue }
    print $i
}
```

### Functions

Declare functions without any keyword — just the name, parameters, and body block:

```xcl
add($a, $b) {
    $a + $b
}

greet($name) {
    return "Hello, " + $name
}

$result = add 3 4
```

Functions support typed parameters and return type annotations:

```xcl
calculate($a: i32, $b: f64) -> f64 {
    $result = $a + $b
    return $result
}
```

**Lambda functions** use pipe syntax only:

```xcl
# Pipe lambda — single param
$square = |$x| $x * $x

# Pipe lambda — multiple params
$add = |$a, $b| $a + $b

# Block lambda — for multi-statement bodies
$complex = { |$x, $y| $x + $y }
```

Lambdas can capture variables from their enclosing scope:

```xcl
$multiplier = 10
$scale = |$x| $x * $multiplier
```

**Note:** All lambda parameters must be declared explicitly.

### Arrays

Arrays are zero-indexed ordered collections:

```xcl
$scores = [3, 2, 5]
print $scores[0]   # 3
print $scores[1]   # 2
```

Negative indices access elements from the end:

```xcl
print $scores[-1]  # 5 (last element)
```

### Maps

Maps store key-value pairs and support dot notation access:

```xcl
$person = { name: "Alice", age: 30 }
print $person.name   # "Alice"
print $person.age    # 30
```

### Custom Types

XCL supports defining custom types through **record types**, **sum types** (tagged unions/enums), and **traits** (interfaces) — all without any special keywords.

**Record types** use assignment syntax to define structured data with named, typed fields:

```xcl
Point = {
    x: f64
    y: f64
}

Person = {
    name: str
    age: i64
}
```

**Sum types** (enums/tagged unions) define a type that can be one of several variants using the `|` separator:

```xcl
# Simple unit variants
Direction = Left | Right | Up | Down

# Enum with associated data
Message = Text { content: str } | Image { url: str, width: u32 } | Empty

# Mixed variants
Command = Stop | Move { x: i32, y: i32 } | Jump { height: f64 }
```

Each variant can carry its own set of fields, or be unit-like (e.g., `Stop` with no fields).

**Traits** (interfaces) are defined as types containing only method signatures (no fields):

```xcl
Drawable = {
    draw() { }
    resize(width: u32, height: u32) { }
}
```

**Trait implementation** uses the `is` keyword. Multiple traits are listed with commas:

```xcl
Clickable = {
    click() { }
}

# Single trait
Button is Drawable {
    draw() {
        print "drawing button"
    }
    resize(w: u32, h: u32) {
        print "resizing"
    }
}

# Multiple traits
Widget is Drawable, Clickable {
    draw() { print "drawing" }
    resize(w: u32, h: u32) { print "resizing" }
    click() { print "clicked" }
}
```

### Type Annotations

Annotate variables and function parameters with explicit types using the `:` syntax:

```xcl
# Variable type annotation
$name: str = "Alice"
$count: i32 = 42

# Function parameter types — params must use $ prefix
greet($name: str, $age: i32) {
    print "Hello, " + $name
}

# Return type annotation
add($a: i32, $b: i32) -> i32 {
    $a + $b
}
```

Type annotations help document intent and enable the compiler to perform type checks.

### Type Testing

Check the runtime type of a value with the `is` operator:

```xcl
if $value is str {
    print "it's a string"
}

if $number is i32 {
    print "it's an integer"
}

# Enum variant test
$dir = Left
if $dir is Left {
    print "going left"
}
```

### Type Casting

Use the `as` operator to cast between types:

```xcl
$raw = 42
$casted = $raw as i32
$float_cast = $raw as f64
```

### Domains

XCL organizes standard library functions into **domains** (namespaces). Functions are called using dot notation:

```xcl
io.print "Hello, world!"
net.https.get "https://api.example.com/data"
encoding.json.parse '{"key": "value"}'
```

### Auto-Import

Certain domains support **auto-import**, allowing their methods to be called as bare identifiers without the domain prefix. This is enabled for commonly-used domains like `io` and `fs`:

```xcl
# These work without domain prefix (auto-import enabled)
print "Hello, world!"      # Same as io.print
$input = input "Name: "    # Same as io.input
$content = read "file.txt" # Same as fs.read
write "out.txt" $content   # Same as fs.write

# These require full qualification (auto-import disabled to avoid conflicts)
net.https.get "https://api.example.com/data"
net.tcp.connect "127.0.0.1" 8080
```

Domains like `net.https` disable auto-import because method names like `get`, `post` are too common and would conflict with other code.

### Domain Expansion

Domain expansion lets you set a context for a block of code, so the domain methods are available without repeating the full path. This is especially useful for cloud providers and services that require configuration.

**Domain blocks** establish a context scope with a configuration value:

```xcl
cloud "aws" "us-east-1" {
    $config = cloud.config
    print "Region:" $config.region
}
```

**Nested domain blocks** allow switching contexts within a scope:

```xcl
cloud "aws" "us-east-1" {
    cloud "aws" "eu-west-1" {
        $config = cloud.config
        print "Nested region:" $config.region  # eu-west-1
    }
    $config = cloud.config
    print "Restored region:" $config.region    # us-east-1
}
```

**Domain values** can be set and retrieved using `domain.run` and `domain.get`:

```xcl
domain.run "bucket.name" "my-test-bucket" {
    $bucket = domain.get "bucket.name"
    print "Bucket context:" $bucket  # my-test-bucket
}
```

**Bare calls** at statement boundaries automatically expand to zero-argument calls:

```xcl
cloud "aws" "us-east-1" {
    $config = cloud.config    # equivalent to cloud.config()
}
```

### Standard Library Domains

| Domain | Purpose | Auto-Import |
|---|---|---|
| `io` | Console I/O: `print`, `println`, `input`, file read/write/seek | ✓ Yes |
| `fs` | Filesystem: `read`, `write`, `exists`, `copy`, `remove`, `mkdir`, `list_dir` | ✓ Yes |
| `net.https` | HTTPS client: `get`, `post`, `put`, `delete`, `patch`, `head`, `options` | ✗ No |
| `net.http` | HTTP client (non-TLS) | ✗ No |
| `net.tcp` | TCP socket: `connect`, `listen`, `accept`, `send`, `recv` | ✗ No |
| `net.udp` | UDP socket: `bind`, `send_to`, `recv_from` | ✗ No |
| `net.tls` | TLS connections | ✗ No |
| `net.dns` | DNS resolution | ✗ No |
| `net.ssh` | SSH connections | ✗ No |
| `encoding.json` | JSON: `parse`, `stringify`, `pretty` | ✗ No |
| `encoding.yaml` | YAML: `parse`, `stringify` | ✗ No |
| `encoding.toml` | TOML: `parse`, `stringify` | ✗ No |
| `encoding.csv` | CSV: `parse`, `stringify` | ✗ No |
| `encoding.xml` | XML: `parse`, `stringify` | ✗ No |
| `cloud` | Cloud providers: AWS, Azure, GCP, Alibaba, OCI, IBM, GitHub | ✗ No |
| `collections.arr` | Array utilities: `map`, `filter`, `reduce`, `sort`, `reverse`, `first`, `last`, `flatten`, `unique`, `chunk`, `zip`, `concat`, `slice`, `contains`, `index_of`, `join`, `sum`, `min`, `max`, `avg` | ✗ No |
| `collections.dict` | Dictionary utilities: `keys`, `values`, `entries`, `get`, `set`, `has`, `size`, `merge`, `filter`, `map` | ✗ No |
| `collections.defer` | Deferred execution | ✗ No |
| `core.result` | Result type: `ok`, `err`, `is_ok`, `is_err`, `unwrap`, `unwrap_or`, `map`, `map_err`, `and_then`, `or_else` | ✗ No |
| `domain` | Domain context management: `run`, `get`, `set`, `clear` | ✗ No |
| `math.scalar` | Scalar math: `sqrt`, `abs`, `min`, `max`, `round`, `floor`, `ceil`, `pow`, `log`, `sin`, `cos`, `tan`, `pi`, `random` | ✗ No |
| `math.complex` | Complex numbers | ✗ No |
| `math.vec` | Vector math | ✗ No |
| `math.mat` | Matrix math | ✗ No |
| `math.quat` | Quaternion math | ✗ No |
| `text.str` | String manipulation: `upper`, `lower`, `trim`, `split`, `join`, `replace`, `contains`, `starts_with`, `ends_with`, `length` | ✗ No |
| `sys.env` | Environment variables: `get`, `set`, `list` | ✗ No |
| `sys.os` | OS information: `name`, `version`, `arch` | ✗ No |
| `sys.process` | Process management: `spawn`, `kill`, `pid` | ✗ No |
| `sys.time` | Time utilities: `now`, `sleep`, `timestamp` | ✗ No |
| `sys.signal` | Signal handling | ✗ No |
| `sys.thread` | Thread management | ✗ No |
| `sys.socket` | Low-level socket operations | ✗ No |
| `sys.syscall` | System calls | ✗ No |
| `ffi` | Foreign Function Interface: `load`, `call`, `symbol`, `free` | ✗ No |
| `lang` | Language introspection | ✗ No |
| `plugin` | Plugin system | ✗ No |
| `proc` | Process execution | ✗ No |
| `util` | Cross-platform utilities | ✗ No |

**Example — I/O (auto-imported):**

```xcl
print "Enter your name: "
$name = input
print "Hello, " + $name
```

**Example — Filesystem (auto-imported):**

```xcl
$content = read "data.txt"
write "output.txt" $content
if exists "config.json" {
    print "Config found"
}
```

**Example — HTTPS:**

```xcl
$response = net.https.get "https://api.example.com/users"
print $response.body
print $response.status
```

**Example — JSON:**

```xcl
$data = { "name": "Alice", "age": 30 }
$json = encoding.json.stringify $data
$parsed = encoding.json.parse $json
```

**Example — Cloud with domain expansion:**

```xcl
cloud "aws" "us-east-1" {
    $config = cloud.config
    print "Provider:" $config.provider
    print "Region:" $config.region
}
```

**Example — Collections:**

```xcl
# Array operations
$squares = collections.arr.map [1, 2, 3, 4, 5] |$x| $x * $x
$evens = collections.arr.filter $squares |$x| $x % 2 == 0

# Pipeline style
$result = [1, 2, 3, 4, 5] |> collections.arr.map |$x| $x * 2
```

**Example — FFI:**

```xcl
# Load a shared library
$lib = ffi.load "libexample.so"

# Call an exported function
$result = ffi.call $lib "my_function"

# Unload library
ffi.free $lib
```

### Result and Error Types

XCL provides `Ok` and `Err` constructors for result handling, along with a `make_error` helper:

```xcl
$result = Ok 42
$error = Err (make_error "not_found" "Resource not found" 404)
```

## Architecture

XCL follows a three-stage compilation pipeline:

```
Source code → Lexer → Parser → Compiler → Bytecode → VM → Output
```

1. **Lexing** — Source code is tokenized by a DFA-based lexer (powered by `logos`). Punctuation and keywords are zero-allocation.
2. **Parsing** — Tokens are parsed into an abstract syntax tree (AST) using a Pratt parser with full support for lambdas, pipelines, type definitions, and context blocks.
3. **Compilation** — The AST is compiled to bytecode instructions.
4. **Execution** — The stack-based VM interprets the bytecode.

### Memory Model

- **NaN-boxing:** All values fit in a single 64-bit word. Primitives use NaN payload bits for tagging; heap objects store pointers in the lower 48 bits.
- **Bump allocation:** Temporary objects are allocated from a bump arena that resets after each loop iteration and function call, providing fast allocation with automatic cleanup.
- **Boxed values:** Long-lived objects use standard heap allocation and survive scope exit.

## Project Structure

| Crate | Description |
|---|---|
| `xcl-vm` | Core VM: lexer, parser, bytecode compiler, and runtime |
| `xcl-macros` | Procedural macros: `#[xcl_fn]` for native functions, `#[derive(Xcl)]` for type registration, `#[xcl_impl]` for impl blocks |
| `xcl-plugins` | Standard library plugins: all domains (io, fs, net, encoding, cloud, ffi, etc.) |
| `xcl-cli` | Command-line interface for running XCL scripts |
| `xcl-debug` | Parser debug tool and bytecode toolkit: tokenize, parse, visualize AST, decompile bytecode, and disassemble (`xcl-debug` and `xcl-bytecode` binaries) |
| `xcl-doc` | Documentation generator that reads macro metadata and produces an HTML site |

## Writing Native Functions

Extend XCL from Rust using the `#[xcl_fn]` procedural macro. The macro auto-detects domain and method name from the function name (e.g., `io_open` → domain=`io`, method=`open`):

```rust
use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Greet a user by name.
#[xcl_fn]
pub fn io_greet(name: &str) -> Value {
    Value::from_str_boxed(format!("Hello, {}!", name))
}
```

For impl blocks, use `#[xcl_impl]` with optional `auto_import` to enable bare-identifier calls:

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

The macros handle registration with the VM at compile time using `linkme`, with zero runtime overhead.

## Parser Debug Tool

The `xcl-debug` tool helps diagnose parser issues by tokenizing and visualizing XCL code:

```bash
# Show token stream
cargo run -p xcl-debug -- --tokens script.xcl

# Show AST tree
cargo run -p xcl-debug -- --tree script.xcl

# Show verbose AST with line:column spans
cargo run -p xcl-debug -- --tree --verbose script.xcl
```

## Documentation Generation

Generate the HTML documentation site from macro metadata:

```bash
cargo run -p xcl-doc --release -- build --out docs
```

The generated site includes domain pages, function reference pages, sidebar navigation, client-side search, and dark mode support.

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

## Building

XCL is written in Rust and supports `no_std` environments. Optimized with trimmed dependencies for minimal binary size (~1 MB release build).

```bash
# Build all crates
cargo build --release

# Run the CLI
cargo run -p xcl-cli -- script.xcl

# Run tests
cargo test --workspace
```

## Examples

See the [`example/`](example/) directory for sample XCL scripts covering all language features, including HTTPS, cloud providers, lambdas, and the comprehensive benchmark.

## Contributing

Contributions are welcome. The most impactful way to contribute is by writing domain plugins — each new domain extends the language's standard library capabilities.

## License

Licensed under either MIT or Apache-2.0.

XCL was created by [Nguyễn Quý Ngọc](https://github.com/ChaoswareHQ).
