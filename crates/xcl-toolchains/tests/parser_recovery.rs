//! Parser error recovery tests — stage 1.

use xcl_toolchain::compiler::lexer::lex;
use xcl_toolchain::compiler::parser::{parse, parse_recover};

// ── 1. Two independent syntax errors yield two errors ──

#[test]
fn two_bad_statements_report_two_errors() {
    // Two bad statements separated by a valid statement.
    // Line 1: stray `)` — triggers E0002
    // Line 2: valid `$x = 1` — parsed successfully
    // Line 3: stray `)` — triggers E0002
    let source = ")\n$x = 1\n)";
    let tokens = lex(source, "test.xcl");
    let out = parse_recover(&tokens, source);
    assert!(
        out.errors.len() >= 2,
        "expected at least 2 errors, got {}: {:?}",
        out.errors.len(),
        out.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    // The two errors should reference different lines.
    let lines: Vec<u32> = out.errors.iter().map(|e| e.span.line).collect();
    assert!(
        lines.contains(&1) && lines.contains(&3),
        "errors should span lines 1 and 3, got: {:?}",
        lines
    );
}

// ── 2. parse() still panics on the first error ──

#[test]
#[should_panic(expected = "unexpected token")]
fn parse_still_panics_on_first_error() {
    let source = ")\n$x = 1\n)";
    let tokens = lex(source, "test.xcl");
    let _ = parse(&tokens, source);
}

// ── 3. Valid program has no errors ──

#[test]
fn valid_program_has_no_errors() {
    let source = "$x = 1 + 2; $x";
    let tokens = lex(source, "test.xcl");
    let out = parse_recover(&tokens, source);
    assert!(
        out.errors.is_empty(),
        "expected no errors, got: {:?}",
        out.errors
    );
    assert!(!out.ast.is_empty(), "expected non-empty AST");
}

// ── 4. Error cap is enforced ──

#[test]
fn error_cap_is_enforced() {
    // 50 stray `>` tokens — each is a separate bad statement.
    let source = (0..50).map(|_| ">").collect::<Vec<_>>().join("\n");
    let tokens = lex(&source, "test.xcl");
    let out = parse_recover(&tokens, &source);
    assert!(
        out.errors.len() <= 10,
        "expected at most 10 errors, got {}",
        out.errors.len()
    );
}

// ── 5. No infinite loop on pathological input ──

#[test]
fn no_infinite_loop_on_stray_tokens() {
    // 50 stray `)` tokens.
    let source = ")".repeat(50);
    let tokens = lex(&source, "test.xcl");
    let out = parse_recover(&tokens, &source);
    assert!(
        !out.errors.is_empty(),
        "expected at least one error for stray ) tokens"
    );
}

#[test]
fn no_infinite_loop_on_nested_braces() {
    // 100 `{` tokens — nested braces.
    let source = "{".repeat(100);
    let tokens = lex(&source, "test.xcl");
    let out = parse_recover(&tokens, &source);
    assert!(
        !out.errors.is_empty(),
        "expected at least one error for 100 opening braces"
    );
}

// ── 6. Help line is rendered ──

#[test]
fn help_is_rendered() {
    // A parameter without $ prefix triggers E0004 with a help suggestion.
    let source = "foo(x) { x }";
    let tokens = lex(source, "test.xcl");
    let out = parse_recover(&tokens, source);
    // Find the error with help text.
    let help_err = out
        .errors
        .iter()
        .find(|e| e.help.is_some())
        .expect("expected an error with help text");
    let rendered = help_err.render();
    assert!(
        rendered.contains("= help:"),
        "rendered error should contain help line, got: {}",
        rendered
    );
}

// ── 8. Nesting depth is bounded by stack headroom, not a fixed count ──

#[test]
fn reasonable_nesting_still_parses() {
    // Deep enough to be a real program, shallow enough for any build.
    let depth = 20;
    let source = format!("print {}1{}", "(".repeat(depth), ")".repeat(depth));
    let tokens = lex(&source, "test.xcl");
    let out = parse_recover(&tokens, &source);
    assert!(
        out.errors.is_empty(),
        "{} levels of nesting should parse, got: {:?}",
        depth,
        out.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    assert!(!out.ast.is_empty());
}

#[test]
fn deep_nesting_reports_an_error_instead_of_crashing() {
    // Without a stack-headroom check this overflows the thread stack and aborts
    // the whole process. A clean diagnostic is the whole point.
    let depth = 2000;
    let source = format!("print {}1{}", "(".repeat(depth), ")".repeat(depth));
    let tokens = lex(&source, "test.xcl");
    let out = parse_recover(&tokens, &source);
    assert!(
        out.errors
            .iter()
            .any(|e| e.message.contains("nesting is too deep")),
        "expected a nesting diagnostic, got: {:?}",
        out.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    assert!(
        out.errors[0].help.is_some(),
        "the nesting diagnostic should suggest a fix"
    );
}

#[test]
fn deep_block_nesting_reports_an_error_instead_of_crashing() {
    let depth = 2000;
    let source = format!("{}{}", "{".repeat(depth), "}".repeat(depth));
    let tokens = lex(&source, "test.xcl");
    let out = parse_recover(&tokens, &source);
    assert!(
        !out.errors.is_empty(),
        "deep block nesting should report an error, not abort the process"
    );
}

// ── 9. Errors at end of input keep their file and position ──

#[test]
fn eof_errors_point_at_the_end_of_the_source() {
    // `if $x >` ends mid-expression: the error is reported at EOF, where there
    // is no token to take a span from. It used to fall back to `Span::empty()`,
    // which renders as `--> :1:1` and points at nothing.
    let source = "$x = 1\nif $x >";
    let tokens = lex(source, "test.xcl");
    let out = parse_recover(&tokens, source);
    let error = out.errors.first().expect("expected a syntax error");
    assert_eq!(error.span.file, "test.xcl", "the file name must survive");
    assert_eq!(error.span.line, 2, "EOF is on line 2, got {:?}", error.span);
    assert!(
        error.span.col > 1,
        "the caret should sit after the last character, got {:?}",
        error.span
    );
}
