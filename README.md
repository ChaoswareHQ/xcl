# XCL

XCL is a scripting language built on a fast, stack-based virtual machine written in Rust. It features NaN-boxed value representation, bump allocation for fast temporary objects, a rich standard library organized into domains, and a pipeline-friendly syntax.

## Quick Start

```bash
# Run a script file
cargo run -p xcl-cli -- script.xcl

# Evaluate an expression inline
cargo run -p xcl-cli -- -e 'print "Hello, world!"'
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
| Function | Callable block | `fn add(a, b) { a + b }` |

**Integer overflow:** When integer arithmetic exceeds 48 bits, XCL automatically falls back to floating-point representation without raising an error.

```xcl
$x = 42
$pi = 3.14159
$flag = true
$empty = none
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

Declare functions with the `fn` keyword. The last expression is returned implicitly, or use `return` for an explicit return.

```xcl
fn add(a, b) {
    a + b
}

fn greet(name) {
    return "Hello, " + name
}

result = add 3 4
```

**Lambda functions** use the `=>` arrow syntax with explicit parameters:

```xcl
$double = ($x) => $x * 2
print $double 21  # 42
```

Lambdas can capture variables from their enclosing scope:

```xcl
$multiplier = 10
$scale = ($x) => $x * $multiplier
```

**Note:** The `$it` implicit parameter has been removed. All lambda parameters must be declared explicitly.

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

XCL supports defining custom types through **record types** and **sum types** (tagged unions).

**Record types** define structured data with named, typed fields:

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

A record with only method definitions becomes a **trait** (interface) automatically. Records with fields are data types.

**Sum types** (tagged unions) define a type that can be one of several variants:

```xcl
Shape = | Circle { r: f64 } | Square { side: f64 } | Line

Message = | Text { content: str } | Image { url: str }
```

Each variant can carry its own set of fields, or be unit-like (e.g., `Line` with no fields). Variants are separated by the `|` pipe operator.

### Traits and Interfaces

A **trait** defines a set of method signatures that a type must implement. When a type definition contains only methods (no fields), XCL treats it as a trait:

```xcl
Drawable = {
    draw() -> none
    area() -> f64
}
```

Implement a trait on a type using an **impl block** with the `is` keyword:

```xcl
Circle is Drawable {
    draw() {
        print "Drawing circle"
    }

    area() {
        math.pi * r * r
    }
}
```

### Type Annotations

Annotate variables and function parameters with explicit types using the `:` syntax:

```xcl
# Variable type annotation
$name: str = "Alice"
$count: i64 = 42

# Function parameter types
fn greet(name: str, age: i64) {
    print "Hello, " + name
}

# Return type annotation
fn add(a: i64, b: i64) -> i64 {
    a + b
}
```

Type annotations help document intent and enable the compiler to perform type checks.

### Type Testing

Check the runtime type of a value with the `is` keyword:

```xcl
if $value is String {
    print "it's a string"
}

if $shape is Circle {
    print "it's a circle with radius " + $shape.r
}
```

### Type Casting

Use the `as` keyword to cast between types. A failed cast returns `none`.

```xcl
$number = "42" as int
$text = 42 as string
```

### Domains

XCL organizes standard library functions into **domains** (namespaces). Functions are called using dot notation:

```xcl
io.print "Hello, world!"
math.sqrt 16
encoding.json.stringify $data
```

Domains can be nested into subdomains:

```xcl
cloud.aws.sign "GET" "https://s3.amazonaws.com/" ""
```

### Domain Expansion

Domain expansion lets you set a context for a block of code, so the domain methods are available without repeating the full path. This is especially useful for cloud providers, databases, and other services that require configuration.

**Domain blocks** establish a context scope with a configuration value:

```xcl
aws "us-east-1" {
    $config = aws.config()
    print "Region:" $config.region
}
```

Inside the block, `aws.config()` returns the configuration for the region set by the block.

**Nested domain blocks** allow switching contexts within a scope. The inner block temporarily overrides the outer context, and the outer context is restored automatically when the inner block exits:

```xcl
aws "us-east-1" {
    aws "eu-west-1" {
        $config = aws.config()
        print "Nested region:" $config.region  # eu-west-1
    }
    $config = aws.config()
    print "Restored region:" $config.region    # us-east-1
}
```

**Domain values** can be set and retrieved using `domain.run` and `domain.get`:

```xcl
aws "us-east-1" {
    domain.run "bucket.name" "my-test-bucket" {
        $bucket = domain.get "bucket.name"
        print "Bucket context:" $bucket  # my-test-bucket
    }
}
```

`domain.run` sets a key-value pair in the domain context for the duration of the block, and `domain.get` retrieves it.

**Cloud provider contexts** use `cloud.run` with the provider name and region as positional string arguments:

```xcl
cloud.run "aws" "us-east-1" {
    $provider = cloud.get
    $config = cloud.config
    print "Provider:" $provider
    print "Region:" $config.region
}
```

**Bare calls** at statement boundaries automatically expand to zero-argument calls. Writing `cloudflare.config` is equivalent to `cloudflare.config()`:

```xcl
cloudflare "" {
    $config = cloudflare.config    # equivalent to cloudflare.config()
    print "Account:" $config.account
}
```

This also works for methods on `$`-variables:

```xcl
$result = bucket.create "test"
if $result.is_ok {       # equivalent to $result.is_ok()
    print "Created"
}
```

**Space-separated arguments** are the only calling style in XCL. Arguments follow the function name directly without parentheses:

```xcl
aws.sign "GET" "https://s3.amazonaws.com/" ""
print "Hello, world!"
```

Parenthesized arguments like `func("arg")` are not allowed — use space-separated form instead.

**Dict literals are not allowed as function arguments.** Configuration must be passed as positional string arguments:

```xcl
# Correct — positional strings
cloud.run "aws" "us-east-1" {
    print "Running in AWS"
}

# Illegal — dict literals as arguments are rejected
# cloud.run "aws" { region: "us-east-1" } { ... }
```

**Method calls on domain results** use dot notation to access properties or call methods on the value returned by a domain function:

```xcl
$response = net.get "https://api.example.com/data"
print $response.body
print $response.status
```

**Cloud provider domains** use domain expansion to manage credentials, regions, and service contexts. Each provider (AWS, Azure, GCP, Cloudflare, Alibaba, etc.) provides its own domain block and a set of service methods:

```xcl
cloudflare "" {
    # R2 bucket operations
    $create = bucket.create "xcl-test-bucket"
    if $create.is_ok {
        print "Bucket created"
    }

    # Workers function operations
    $workers = function.list
    if $workers.is_ok {
        print "Workers list: OK"
    }
}
```

### Standard Library Domains

| Domain | Purpose |
|---|---|
| `io` | Input/output: `print`, `input`, file operations |
| `math` | Mathematics: `sqrt`, `pi`, `random`, complex numbers, matrices |
| `encoding` | Data formats: JSON, YAML, TOML, XML, CSV, hex |
| `net` | Networking: TCP, UDP, HTTP, DNS, TLS |
| `cloud` | Cloud providers: AWS (S3, Lambda, API Gateway) |
| `text` | String manipulation and processing |
| `collections` | Array and dictionary utilities |
| `util` | Cross-platform utilities: OS, environment, paths, time |
| `core` | Foundational types: `Error`, `Result`, traits |
| `lang` | Language introspection and support |

**Example — I/O:**

```xcl
io.print "Enter your name: "
$name = io.input()
io.print "Hello, " + $name
```

**Example — Math:**

```xcl
math.sqrt 16          # 4.0
math.pi               # 3.14159...
math.random           # a random number
```

**Example — Encoding:**

```xcl
$json = encoding.json.stringify $data
$parsed = encoding.json.parse $jsonString
```

**Example — Networking:**

```xcl
$response = net.get "https://api.example.com/data"
$body = $response.body
```

**Example — Cloud with domain expansion:**

```xcl
aws "us-east-1" {
    $config = aws.config()
    $signed = aws.sign "GET" "https://s3.amazonaws.com/" ""

    domain.run "bucket.name" "my-bucket" {
        $bucket = domain.get "bucket.name"
        print "Working with bucket:" $bucket
    }
}

# Or use cloud.run directly with positional args
cloud.run "aws" "us-east-1" {
    $provider = cloud.get
    print "Provider:" $provider
}
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
2. **Parsing** — Tokens are parsed into an abstract syntax tree (AST).
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
| `xcl-macros` | Procedural macros: `#[xcl_fn]` for native functions, `#[xcl_type]` for type descriptors |
| `xcl-plugins` | Standard library plugins: `io`, `math`, `encoding`, `net`, `cloud`, and more |
| `xcl-cli` | Command-line interface for running XCL scripts |
| `xcl-doc` | Documentation generator that reads macro metadata and produces an HTML site |
| `xcl-decompile` | Bytecode decompiler for inspecting compiled programs |

## Writing Native Functions

Extend XCL from Rust using the `#[xcl_fn]` procedural macro:

```rust
use xcl_macros::xcl_fn;
use xcl_vm::Value;

#[xcl_fn(name = "greet", domain = "std", args = "name: str", ret = "str")]
pub fn greet_fn(name: &str) -> Value {
    Value::from_str_boxed(format!("Hello, {}!", name))
}
```

The macro handles registration with the VM at compile time using `linkme`, with zero runtime overhead.

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

XCL is written in Rust and supports `no_std` environments.

```bash
# Build all crates
cargo build --release

# Run the CLI
cargo run -p xcl-cli -- script.xcl

# Run tests
cargo test
```

## Examples

See the [`example/`](example/) directory for sample XCL scripts, including cloud provider integration examples.

## Contributing

Contributions are welcome. The most impactful way to contribute is by writing domain plugins — each new domain extends the language's standard library capabilities.

## License

Licensed under either MIT or Apache-2.0.

XCL was created by [Nguyễn Quý Ngọc](https://github.com/ChaoswareHQ).
