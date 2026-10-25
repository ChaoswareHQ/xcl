//! Parser debug tool for XCL — visualizes token stream, AST tree, and errors.
//!
//! # Usage
//!
//! ```bash
//! # Show tokens only
//! xcl-debug --tokens file.xcl
//!
//! # Show AST tree
//! xcl-debug --ast file.xcl
//!
//! # Show everything (tokens + AST + stats)
//! xcl-debug file.xcl
//!
//! # Evaluate an expression
//! xcl-debug -e "$x = 42; $x + 1"
//!
//! # Verbose mode — shows AST with line numbers
//! xcl-debug --verbose file.xcl
//!
//! # Pretty-print AST as a tree
//! xcl-debug --tree file.xcl
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process;

use xcl_toolchain::AstNode;
use xcl_toolchain::Span;
use xcl_toolchain::lexer::{self, TokenWithText};
use xcl_toolchain::parser;

/// Extract the span from any AST node
fn node_span(node: &AstNode) -> &Span {
    match node {
        AstNode::Assign { span, .. }
        | AstNode::Return { span, .. }
        | AstNode::Break { span, .. }
        | AstNode::Continue { span, .. }
        | AstNode::If { span, .. }
        | AstNode::For { span, .. }
        | AstNode::While { span, .. }
        | AstNode::FunctionDef { span, .. }
        | AstNode::Lambda { span, .. }
        | AstNode::Call { span, .. }
        | AstNode::DomainCall { span, .. }
        | AstNode::Ident { span, .. }
        | AstNode::Int { span, .. }
        | AstNode::Float { span, .. }
        | AstNode::Str { span, .. }
        | AstNode::Bool { span, .. }
        | AstNode::Nil { span, .. }
        | AstNode::BinaryOp { span, .. }
        | AstNode::UnaryOp { span, .. }
        | AstNode::Pipeline { span, .. }
        | AstNode::ArrayLit { span, .. }
        | AstNode::DictLit { span, .. }
        | AstNode::Index { span, .. }
        | AstNode::FieldAccess { span, .. }
        | AstNode::SetField { span, .. }
        | AstNode::SetIndex { span, .. }
        | AstNode::Range { span, .. }
        | AstNode::Block { span, .. }
        | AstNode::TypeDef { span, .. }
        | AstNode::ImplDef { span, .. }
        | AstNode::TypeTest { span, .. }
        | AstNode::TypeCast { span, .. }
        | AstNode::SpecialVar { span, .. } => span,
    }
}

struct CliOpts {
    help: bool,
    tokens: bool,
    ast: bool,
    tree: bool,
    verbose: bool,
    eval: Option<String>,
    files: Vec<String>,
}

fn parse_args<I>(iter: I) -> CliOpts
where
    I: IntoIterator<Item = String>,
{
    let mut opts = CliOpts {
        help: false,
        tokens: false,
        ast: false,
        tree: false,
        verbose: false,
        eval: None,
        files: Vec::new(),
    };
    let mut args = iter.into_iter();
    args.next();

    while let Some(arg) = args.next() {
        if arg == "--" {
            opts.files.extend(args.by_ref());
            break;
        }
        if arg.starts_with("--") {
            match arg.as_str() {
                "--help" => opts.help = true,
                "--tokens" => opts.tokens = true,
                "--ast" => opts.ast = true,
                "--tree" => {
                    opts.tree = true;
                    opts.ast = true;
                }
                "--verbose" => opts.verbose = true,
                "--eval" => {
                    opts.eval = args.next().or_else(|| {
                        eprintln!("xcl-debug: error: --eval requires an argument");
                        Some(String::new())
                    });
                }
                unknown => {
                    eprintln!("xcl-debug: error: unknown option '{}'", unknown);
                    process::exit(1);
                }
            }
            continue;
        }
        if arg.starts_with("-") && arg.len() > 1 {
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut i = 0;
            while i < chars.len() {
                match chars[i] {
                    'h' => opts.help = true,
                    't' => opts.tokens = true,
                    'a' => opts.ast = true,
                    'v' => opts.verbose = true,
                    'e' => {
                        let val = if i + 1 < chars.len() {
                            chars[i + 1..].iter().collect::<String>()
                        } else if let Some(next) = args.next() {
                            next
                        } else {
                            eprintln!("xcl-debug: error: -e requires an argument");
                            process::exit(1);
                        };
                        opts.eval = Some(val);
                        i += 1;
                    }
                    unknown => {
                        eprintln!("xcl-debug: error: unknown option '-{}'", unknown);
                        process::exit(1);
                    }
                }
                i += 1;
            }
            continue;
        }
        opts.files.push(arg);
    }
    opts
}

fn print_help() {
    println!(
        "Usage: xcl-debug [OPTIONS] [FILE]...

Parser debug tool for XCL — inspect tokens, AST, and parse errors.

Options:
  -h, --help          Show this help message
  -t, --tokens        Show only the token stream
  -a, --ast           Show only the AST (Debug format)
  --tree              Show AST as an indented tree
  -v, --verbose       Show tokens with line numbers and AST with spans
  -e, --eval <EXPR>   Evaluate the given expression string

Modes:
  If no mode flag is given, shows tokens + AST + statistics.
  With --tokens, shows only the token stream.
  With --ast or --tree, shows only the AST.

Examples:
  xcl-debug script.xcl              # Show tokens + AST + stats
  xcl-debug --tokens script.xcl     # Show only tokens
  xcl-debug --tree script.xcl       # Show AST as a tree
  xcl-debug -e \"$x = 42; $x + 1\"  # Debug an expression
"
    );
}

fn print_tokens(tokens: &[TokenWithText], verbose: bool) {
    println!("=== TOKEN STREAM ({}) ===", tokens.len());
    println!();
    for (i, tok) in tokens.iter().enumerate() {
        if verbose {
            println!(
                "{:4} | {:>4}:{:<4} | {:<20} | {:?}",
                i,
                tok.span.line,
                tok.span.col,
                format!("\"{}\"", tok.text),
                tok.token
            );
        } else {
            println!(
                "{:4} | {:<20} | {:?}",
                i,
                format!("\"{}\"", tok.text),
                tok.token
            );
        }
    }
    println!();
}

fn node_name(node: &AstNode) -> &'static str {
    match node {
        AstNode::Assign { .. } => "Assign",
        AstNode::Return { .. } => "Return",
        AstNode::Break { .. } => "Break",
        AstNode::Continue { .. } => "Continue",
        AstNode::If { .. } => "If",
        AstNode::For { .. } => "For",
        AstNode::While { .. } => "While",
        AstNode::FunctionDef { .. } => "FnDef",
        AstNode::Lambda { .. } => "Lambda",
        AstNode::Call { .. } => "Call",
        AstNode::DomainCall { .. } => "DomainCall",
        AstNode::Ident { .. } => "Ident",
        AstNode::Int { .. } => "Int",
        AstNode::Float { .. } => "Float",
        AstNode::Str { .. } => "Str",
        AstNode::Bool { .. } => "Bool",
        AstNode::Nil { .. } => "Nil",
        AstNode::BinaryOp { .. } => "BinOp",
        AstNode::UnaryOp { .. } => "UnOp",
        AstNode::Pipeline { .. } => "Pipeline",
        AstNode::ArrayLit { .. } => "Array",
        AstNode::DictLit { .. } => "Dict",
        AstNode::Index { .. } => "Index",
        AstNode::FieldAccess { .. } => "Field",
        AstNode::SetField { .. } => "SetField",
        AstNode::SetIndex { .. } => "SetIndex",
        AstNode::Range { .. } => "Range",
        AstNode::Block { .. } => "Block",
        AstNode::TypeDef { .. } => "TypeDef",
        AstNode::ImplDef { .. } => "ImplDef",
        AstNode::TypeTest { .. } => "TypeTest",
        AstNode::TypeCast { .. } => "TypeCast",
        AstNode::SpecialVar { .. } => "SpecialVar",
    }
}

/// Collect all child nodes from an AST node for recursive printing
fn children(node: &AstNode) -> Vec<AstNode> {
    let mut kids = Vec::new();
    match node {
        AstNode::Assign { value, .. } => kids.push((**value).clone()),
        AstNode::Return { value: Some(v), .. } => kids.push((**v).clone()),
        AstNode::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            kids.push((**condition).clone());
            kids.extend(then_branch.clone());
            if let Some(else_b) = else_branch {
                kids.extend(else_b.clone());
            }
        }
        AstNode::For { iterable, body, .. } => {
            kids.push((**iterable).clone());
            kids.extend(body.clone());
        }
        AstNode::While {
            condition, body, ..
        } => {
            kids.push((**condition).clone());
            kids.extend(body.clone());
        }
        AstNode::FunctionDef { body, .. } => kids.extend(body.clone()),
        AstNode::Lambda { body, .. } => kids.push((**body).clone()),
        AstNode::Call { callee, args, .. } => {
            kids.push((**callee).clone());
            for arg in args {
                kids.push(arg.value.clone());
            }
        }
        AstNode::DomainCall {
            callee,
            args,
            items,
            ..
        } => {
            kids.push((**callee).clone());
            for arg in args {
                kids.push(arg.value.clone());
            }
            for item in items {
                kids.push(item.value.clone());
            }
        }
        AstNode::BinaryOp { left, right, .. } => {
            kids.push((**left).clone());
            kids.push((**right).clone());
        }
        AstNode::UnaryOp { operand, .. } => kids.push((**operand).clone()),
        AstNode::Pipeline { input, stages, .. } => {
            kids.push((**input).clone());
            for stage in stages {
                if let Some(arg) = &stage.arg {
                    kids.push((**arg).clone());
                }
            }
        }
        AstNode::ArrayLit { elements, .. } => kids.extend(elements.clone()),
        AstNode::DictLit { entries, .. } => {
            for (_, val) in entries {
                kids.push(val.clone());
            }
        }
        AstNode::Index { target, index, .. } => {
            kids.push((**target).clone());
            kids.push((**index).clone());
        }
        AstNode::FieldAccess { target, .. } => kids.push((**target).clone()),
        AstNode::Block { statements, .. } => kids.extend(statements.clone()),
        AstNode::TypeDef { body, .. } => kids.extend(body.clone()),
        AstNode::ImplDef { body, .. } => kids.extend(body.clone()),
        AstNode::TypeTest { value, .. } => kids.push((**value).clone()),
        AstNode::TypeCast { value, .. } => kids.push((**value).clone()),
        AstNode::Range { start, end, .. } => {
            if let Some(s) = start {
                kids.push((**s).clone());
            }
            if let Some(e) = end {
                kids.push((**e).clone());
            }
        }
        _ => {}
    }
    kids
}

fn print_tree(nodes: &[AstNode], indent: usize, verbose: bool) {
    for node in nodes {
        let name = node_name(node);
        let span = node_span(node);
        let span_info = if verbose {
            format!(" @{}:{}", span.line, span.col)
        } else {
            String::new()
        };

        let prefix = " ".repeat(indent);
        print!("{}├──{}{}", prefix, name, span_info);

        // Print key info inline
        match node {
            AstNode::Assign {
                name,
                type_annotation,
                ..
            } => {
                print!(" ${}", name);
                if let Some(ty) = type_annotation {
                    print!(" :{}", ty);
                }
            }
            AstNode::Ident { name, .. } => print!(" {}", name),
            AstNode::Int { value, .. } => print!(" {}", value),
            AstNode::Float { value, .. } => print!(" {}", value),
            AstNode::Str { value, .. } => {
                let truncated = if value.len() > 30 {
                    format!("\"{}...\"", &value[..27])
                } else {
                    format!("\"{}\"", value)
                };
                print!(" {}", truncated);
            }
            AstNode::Bool { value, .. } => print!(" {}", value),
            AstNode::BinaryOp { op, .. } => print!(" {:?}", op),
            AstNode::UnaryOp { op, .. } => print!(" {:?}", op),
            AstNode::Call { callee, args, .. } => {
                let callee_name = match callee.as_ref() {
                    AstNode::Ident { name, .. } => name.clone(),
                    AstNode::FieldAccess { field, .. } => field.clone(),
                    _ => node_name(callee).to_string(),
                };
                print!(" {}({} args)", callee_name, args.len());
            }
            AstNode::DomainCall {
                callee,
                args,
                items,
                ..
            } => {
                let callee_name = match callee.as_ref() {
                    AstNode::Ident { name, .. } => name.clone(),
                    AstNode::FieldAccess { field, .. } => field.clone(),
                    _ => node_name(callee).to_string(),
                };
                print!(
                    " {}({} args -> {} items)",
                    callee_name,
                    args.len(),
                    items.len()
                );
            }
            AstNode::FunctionDef { name, params, .. } => {
                print!(" {}({} params)", name, params.len());
            }
            AstNode::Lambda { params, .. } => print!(" ({} params)", params.len()),
            AstNode::ArrayLit { elements, .. } => print!(" [{} elems]", elements.len()),
            AstNode::DictLit { entries, .. } => print!(" {{{} entries}}", entries.len()),
            AstNode::Block { statements, .. } => print!(" {{{} stmts}}", statements.len()),
            AstNode::If {
                else_branch: Some(else_b),
                ..
            } => {
                print!(" (else: {} stmts)", else_b.len());
            }
            AstNode::If { .. } => print!(" (no else)"),
            AstNode::For { variable, .. } => print!(" for {}", variable),
            AstNode::While { .. } => print!(" while"),
            AstNode::Return { value: Some(_), .. } => print!(" (with value)"),
            AstNode::Return { .. } => print!(" ()"),
            AstNode::TypeDef { name, .. } => print!(" {}", name),
            AstNode::FieldAccess { field, .. } => print!(" .{}", field),
            AstNode::Index { .. } => print!(" []"),
            AstNode::Pipeline { stages, .. } => print!(" ({} stages)", stages.len()),
            AstNode::Range { .. } => print!(" .."),
            AstNode::ImplDef {
                type_name,
                trait_name,
                ..
            } => {
                print!(" {} is {}", type_name, trait_name);
            }
            AstNode::TypeTest { type_name, .. } => print!(" is {}", type_name),
            AstNode::TypeCast { type_name, .. } => print!(" as {}", type_name),
            _ => {}
        }
        println!();

        // Recurse into children
        let child_indent = indent + 2;
        let kids = children(node);
        if !kids.is_empty() {
            print_tree(&kids, child_indent, verbose);
        }
    }
}

fn count_nodes(nodes: &[AstNode]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for node in nodes {
        let name = node_name(node).to_string();
        *counts.entry(name).or_insert(0) += 1;
        let kids = children(node);
        let sub = count_nodes(&kids);
        for (k, v) in sub {
            *counts.entry(k).or_insert(0) += v;
        }
    }
    counts
}

fn print_stats(nodes: &[AstNode]) {
    let counts = count_nodes(nodes);
    let total: usize = counts.values().sum();
    println!("=== AST STATISTICS ===");
    println!("Top-level statements: {}", nodes.len());
    println!("Total nodes: {}", total);
    println!();
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    println!("Node type distribution:");
    for (name, count) in &sorted {
        let bar = "#".repeat(*count / 2);
        println!("  {:<12} {:>3} {}", name, count, bar);
    }
    println!();
}

fn debug_source(source: &str, file: &str, opts: &CliOpts) -> Result<(), String> {
    let tokens = lexer::lex(source, file);

    let show_tokens = opts.tokens || !opts.ast;
    let show_ast = opts.ast || !opts.tokens;

    if show_tokens {
        print_tokens(&tokens, opts.verbose);
    }

    if show_ast {
        let ast = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            parser::parse(&tokens, source)
        })) {
            Ok(ast) => ast,
            Err(_) => {
                eprintln!();
                eprintln!("❌ PARSER PANIC in {}:", file);
                eprintln!();
                eprintln!("The parser encountered unexpected tokens.");
                eprintln!("Check the token stream above for clues.");
                eprintln!();
                if tokens.len() > 10 {
                    eprintln!("Last 10 tokens:");
                    for (i, tok) in tokens.iter().skip(tokens.len() - 10).enumerate() {
                        eprintln!(
                            "  {:4} | {:<20} | {:?}",
                            i,
                            format!("\"{}\"", tok.text),
                            tok.token
                        );
                    }
                } else if !tokens.is_empty() {
                    eprintln!("All tokens:");
                    for (i, tok) in tokens.iter().enumerate() {
                        eprintln!(
                            "  {:4} | {:<20} | {:?}",
                            i,
                            format!("\"{}\"", tok.text),
                            tok.token
                        );
                    }
                }
                return Err("Parser panic".to_string());
            }
        };

        if opts.tree {
            println!("=== AST TREE ===");
            println!();
            print_tree(&ast, 0, opts.verbose);
            println!();
        } else {
            println!("=== AST (Debug) ===");
            println!();
            println!("{:#?}", ast);
            println!();
        }

        if !opts.tokens && !opts.ast {
            print_stats(&ast);
        }
    }

    Ok(())
}

fn main() {
    let opts = parse_args(env::args());

    if opts.help {
        print_help();
        process::exit(0);
    }

    if opts.files.is_empty() && opts.eval.is_none() {
        eprintln!("xcl-debug: error: no file or expression provided");
        eprintln!("Use --help for usage information.");
        process::exit(1);
    }

    let mut exit_code = 0;

    if let Some(ref expr) = opts.eval {
        eprintln!("Debugging expression:");
        eprintln!("  {}", expr);
        eprintln!();
        if let Err(e) = debug_source(expr, "<eval>", &opts) {
            eprintln!("Error: {}", e);
            exit_code = 1;
        }
    }

    for file in &opts.files {
        let source = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("xcl-debug: error: cannot read '{}': {}", file, e);
                exit_code = 1;
                continue;
            }
        };

        eprintln!("Debugging file: {}", file);
        eprintln!(
            "Source size: {} bytes, {} lines",
            source.len(),
            source.lines().count()
        );
        eprintln!();

        if let Err(e) = debug_source(&source, file, &opts) {
            eprintln!("Error: {}", e);
            exit_code = 1;
        }

        eprintln!();
        eprintln!("────────────────────────────────────────");
        eprintln!();
    }

    process::exit(exit_code);
}
