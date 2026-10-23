//! Import resolution for the CLI.
//!
//! A top-level line of the form
//!
//! ```xcl
//! hello = test.hello
//! ```
//!
//! loads `test.xcl` relative to the importing file and makes its top-level
//! function `hello` available under the name `hello`. The right-hand side is a
//! dotted path: every segment but the last names the module file, the last is
//! the symbol. A different local name is allowed (`greet = test.hello`).
//!
//! Import lines are removed before parsing, so the compiler and the linter see
//! ordinary XCL. Each file is parsed separately, which keeps diagnostic spans
//! pointing at the file they came from. Imported definitions are placed before
//! the importing file in the assembled AST; a module is included once even when
//! several files import it.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use xcl_vm::lexer::{Token, lex};
use xcl_vm::{AstNode, ParseError, Span};

/// Why a program could not be assembled.
pub enum Failure {
    /// Syntax errors in the entry file or in an imported module.
    Parse {
        /// File the errors came from.
        file: String,
        /// Original source, for rendering the diagnostics.
        source: String,
        /// Every error the parser recovered from.
        errors: Vec<ParseError>,
    },
    /// An import could not be resolved. `error.span` points at the import.
    Resolve {
        /// Source of the file containing the import.
        source: String,
        /// The resolution error.
        error: ParseError,
    },
}

/// One `binding = module.symbol` statement.
#[derive(Clone)]
struct ImportSpec {
    binding: String,
    module: String,
    symbol: String,
    span: Span,
}

/// A parsed file plus the imports found in it.
#[derive(Clone)]
struct ModuleSource {
    /// Path reported in diagnostics.
    display: String,
    /// Path used to resolve this module's own imports.
    path: Option<PathBuf>,
    /// Canonical path, for cycle detection and deduplication.
    key: Option<PathBuf>,
    /// Original source.
    source: String,
    /// Top-level statements with import lines removed.
    ast: Vec<AstNode>,
    /// Imports declared by this file.
    imports: Vec<ImportSpec>,
}

#[derive(Default)]
struct Loader {
    cache: HashMap<PathBuf, ModuleSource>,
    /// Modules currently being expanded, for cycle detection.
    stack: Vec<PathBuf>,
    /// Modules already placed in the output.
    included: HashSet<PathBuf>,
    /// `(module, symbol, binding)` aliases already emitted.
    aliases: HashSet<(PathBuf, String, String)>,
    out: Vec<AstNode>,
}

/// Parses `source` (the entry file) and every module it imports, returning one
/// AST in dependency order.
///
/// `entry_path` is `None` for `-e` and stdin input; imports then resolve
/// relative to the current directory.
pub fn load(
    entry_path: Option<&Path>,
    entry_name: &str,
    source: &str,
) -> Result<Vec<AstNode>, Failure> {
    let mut loader = Loader::default();
    let entry = parse_source(entry_path, entry_name, source)?;
    loader.expand(&entry, None)?;
    Ok(loader.out)
}

/// Removes import lines from `source`, leaving line numbers untouched.
///
/// The linter runs on source text rather than the assembled AST, so it uses
/// this to avoid reporting an import as a syntax error.
pub fn strip_imports(source: &str, file: &str) -> String {
    scan_imports(source, file).0
}

impl Loader {
    fn expand(
        &mut self,
        module: &ModuleSource,
        imported_by: Option<(&ImportSpec, &str)>,
    ) -> Result<(), Failure> {
        if let Some(key) = &module.key {
            if self.stack.contains(key) {
                let (spec, source) = imported_by.expect("a cycle has an importing statement");
                return Err(import_error(
                    spec,
                    source,
                    format!("circular import of `{}`", module.display),
                    Some(
                        "move the shared definition into a third module that both files import"
                            .to_string(),
                    ),
                ));
            }
            if self.included.contains(key) {
                return Ok(());
            }
            self.stack.push(key.clone());
        }

        let base = module
            .path
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));

        for spec in &module.imports {
            let dependency = self.load_dependency(base, spec, &module.source)?;
            self.expand(&dependency, Some((spec, &module.source)))?;
            self.bind(&dependency, spec, &module.source)?;
        }

        if let Some(key) = &module.key {
            self.stack.pop();
            self.included.insert(key.clone());
        }
        self.out.extend(module.ast.iter().cloned());
        Ok(())
    }

    /// Resolves `spec` to a module file, parsing it once and caching the result.
    fn load_dependency(
        &mut self,
        base: &Path,
        spec: &ImportSpec,
        importing_source: &str,
    ) -> Result<ModuleSource, Failure> {
        let relative = spec.module.replace('.', "/");
        let mut candidates = vec![base.join(&relative).with_extension("xcl")];
        // Also accept `pkg.test` meaning `pkg.test.xcl`, not only `pkg/test.xcl`.
        let flat = base.join(format!("{}.xcl", spec.module));
        if !candidates.contains(&flat) {
            candidates.push(flat);
        }

        let Some(path) = candidates.iter().find(|path| path.is_file()) else {
            let tried = candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join("`, `");
            return Err(import_error(
                spec,
                importing_source,
                format!("cannot find module `{}` (tried `{}`)", spec.module, tried),
                Some(format!(
                    "expected a file next to the importing source, e.g. `{}.xcl`",
                    spec.module.replace('.', "/")
                )),
            ));
        };

        let key = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        if let Some(module) = self.cache.get(&key) {
            return Ok(module.clone());
        }

        let source = fs::read_to_string(path).map_err(|err| {
            import_error(
                spec,
                importing_source,
                format!("cannot read `{}`: {}", path.display(), err),
                None,
            )
        })?;
        let display = path.display().to_string();
        let module = parse_source(Some(path), &display, &source)?;
        self.cache.insert(key, module.clone());
        Ok(module)
    }

    /// Ensures `spec.binding` names a definition exported by `module`.
    ///
    /// The module is already in the output, so a same-name binding needs no
    /// work. A different binding clones the function definition under the new
    /// name; this keeps recursive and helper calls inside the module working.
    fn bind(
        &mut self,
        module: &ModuleSource,
        spec: &ImportSpec,
        importing_source: &str,
    ) -> Result<(), Failure> {
        let function = module
            .ast
            .iter()
            .find(|node| matches!(node, AstNode::FunctionDef { name, .. } if *name == spec.symbol));
        let is_type = module.ast.iter().any(|node| {
            matches!(
                node,
                AstNode::TypeDef { name, .. } if *name == spec.symbol
            )
        });

        if function.is_none() && !is_type {
            return Err(import_error(
                spec,
                importing_source,
                format!(
                    "`{}` has no function or type named `{}`",
                    module.display, spec.symbol
                ),
                Some(format!(
                    "`{}` exports only its top-level functions and types",
                    module.display
                )),
            ));
        }
        if spec.binding == spec.symbol {
            return Ok(());
        }
        let Some(AstNode::FunctionDef {
            span,
            params,
            return_type,
            body,
            ..
        }) = function
        else {
            return Err(import_error(
                spec,
                importing_source,
                format!(
                    "`{}` is a type; import it as `{} = {}.{}`",
                    spec.symbol, spec.symbol, spec.module, spec.symbol
                ),
                None,
            ));
        };

        let key = module.key.clone().unwrap_or_default();
        if !self
            .aliases
            .insert((key, spec.symbol.clone(), spec.binding.clone()))
        {
            return Ok(());
        }
        self.out.push(AstNode::FunctionDef {
            span: span.clone(),
            name: spec.binding.clone(),
            params: params.clone(),
            return_type: return_type.clone(),
            body: body.clone(),
        });
        Ok(())
    }
}

fn parse_source(path: Option<&Path>, display: &str, source: &str) -> Result<ModuleSource, Failure> {
    let (stripped, imports) = scan_imports(source, display);
    let tokens = lex(&stripped, display);
    let parsed = xcl_vm::parse_recover(&tokens, &stripped);
    if !parsed.errors.is_empty() {
        return Err(Failure::Parse {
            file: display.to_string(),
            source: source.to_string(),
            errors: parsed.errors,
        });
    }

    let key = path.and_then(|path| fs::canonicalize(path).ok());
    Ok(ModuleSource {
        display: display.to_string(),
        path: path.map(Path::to_path_buf),
        key,
        source: source.to_string(),
        ast: parsed.ast,
        imports,
    })
}

/// Finds `binding = module.symbol` statements and blanks them out.
///
/// Only top-level statements are considered: the token walk tracks bracket
/// depth, so a matching line inside a function body stays ordinary code.
fn scan_imports(source: &str, file: &str) -> (String, Vec<ImportSpec>) {
    let tokens = lex(source, file);
    let mut imports = Vec::new();
    let mut blanked = source.as_bytes().to_vec();
    let mut depth = 0i32;
    let mut i = 0;

    while i < tokens.len() {
        match tokens[i].token {
            Token::LBrace | Token::LParen | Token::LBracket => depth += 1,
            Token::RBrace | Token::RParen | Token::RBracket => {
                depth = (depth - 1).max(0);
            }
            _ => {}
        }

        if depth == 0
            && tokens[i].token == Token::IdentPlain
            && matches!(tokens.get(i + 1).map(|t| &t.token), Some(Token::Assign))
            && matches!(tokens.get(i + 2).map(|t| &t.token), Some(Token::IdentPlain))
        {
            let mut parts = vec![tokens[i + 2].text.clone()];
            let mut j = i + 3;
            while j + 1 < tokens.len()
                && tokens[j].token == Token::Dot
                && tokens[j + 1].token == Token::IdentPlain
            {
                parts.push(tokens[j + 1].text.clone());
                j += 2;
            }

            if parts.len() >= 2 && ends_statement(&tokens, j) {
                let end = if tokens
                    .get(j)
                    .map(|t| t.token == Token::Semi)
                    .unwrap_or(false)
                {
                    j + 1
                } else {
                    j
                };
                let start_byte = tokens[i].byte_offset;
                let end_byte = tokens[end - 1].byte_offset + tokens[end - 1].text.len();
                for byte in &mut blanked[start_byte..end_byte] {
                    if !matches!(*byte, b'\n' | b'\r') {
                        *byte = b' ';
                    }
                }

                let symbol = parts.pop().expect("at least two path segments");
                imports.push(ImportSpec {
                    binding: tokens[i].text.clone(),
                    module: parts.join("."),
                    symbol,
                    span: tokens[i].span.clone(),
                });
                i = end;
                continue;
            }
        }

        i += 1;
    }

    // Every rewritten byte is ASCII, so the buffer is still valid UTF-8.
    let stripped = String::from_utf8(blanked).unwrap_or_else(|_| source.to_string());
    (stripped, imports)
}

/// True when nothing else is written after the path on the same line.
fn ends_statement(tokens: &[xcl_vm::lexer::TokenWithText], next: usize) -> bool {
    match tokens.get(next) {
        None => true,
        Some(token) if token.token == Token::Semi => true,
        Some(token) => token.span.line > tokens[next - 1].span.line,
    }
}

fn import_error(spec: &ImportSpec, source: &str, message: String, help: Option<String>) -> Failure {
    Failure::Resolve {
        source: source.to_string(),
        error: ParseError {
            code: "E0010",
            message,
            span: spec.span.clone(),
            help,
        },
    }
}
