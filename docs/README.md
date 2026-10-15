# XCL Standard Library

Auto-generated documentation for XCL built-in functions and types.

## Table of Contents

- [Global](#global)
- [io](#io)

## Global

### Functions

#### `Ok`

**Signature:**

```xcl
Ok(value: any) -> Result
```


#### `Err`

**Signature:**

```xcl
Err(message: str; code: i64) -> Result
```


### Types

#### `Result`

A success or failure. Construct with Ok(value) or Err(message, code). Destructure with match, or by checking $r.tag.

**Kind:** Sum

**Variants:**

##### `Ok`

| Name | Type | Optional |
|---|---|---|
| `value` | `any` | no |

##### `Err`

| Name | Type | Optional |
|---|---|---|
| `message` | `str` | no |
| `code` | `i64` | no |


## io

### Functions

#### `io.eof`


#### `io.copy`

**Signature:**

```xcl
io.copy(src: str; dst: str) -> Result
```


#### `io.open`

**Signature:**

```xcl
io.open(path: str; mode: str) -> Result
```


#### `io.read`

**Signature:**

```xcl
io.read(path: str) -> Result
```


#### `io.seek`


#### `io.tell`


#### `io.close`


#### `io.flush`


#### `io.lines`

**Signature:**

```xcl
io.lines(path: str) -> Result
```


#### `io.stdin`


#### `io.write`

**Signature:**

```xcl
io.write(path: str; data: str) -> Result
```


#### `io.append`

**Signature:**

```xcl
io.append(path: str; data: str) -> Result
```


#### `io.is_tty`


#### `io.stderr`


#### `io.stdout`


#### `io.read_all`

**Signature:**

```xcl
io.read_all(path: str) -> str
```


#### `io.temp_dir`


#### `io.write_to`


#### `io.read_from`


#### `io.read_line`


#### `io.temp_file`

**Signature:**

```xcl
io.temp_file(suffix: str) -> str
```


#### `io.to_string`


#### `io.read_bytes`

**Signature:**

```xcl
io.read_bytes(path: str) -> Result
```


#### `io.write_bytes`


#### `io.write_str_to`


#### `io.read_all_from`


#### `io.write_line_to`


#### `io.read_line_from`


#### `io.print`

**Returns:** `none`


#### `io.format`

**Returns:** `str`


#### `io.input`

**Returns:** `str?`


#### `io.input_all`

**Returns:** `str`


#### `io.input_int`

**Returns:** `i64?`


#### `io.input_bool`

**Returns:** `bool?`


#### `io.input_line`

**Returns:** `str?`


#### `io.input_word`

**Returns:** `str?`


#### `io.input_float`

**Returns:** `f64?`


### Types

#### `File`

**Kind:** Opaque


#### `Stream`

**Kind:** Opaque


