//! Renders one program's diagnostics in both forms.
//!
//! ```sh
//! cargo run -p xcl-analysis --example report
//! ```

use xcl_toolchain::lint::{SourceMap, diagnostics};

const SOURCE: &str = r#"
$unused = 1

handler($event) {
    print "handled"
}

for $i in 0..3 {
}

if true {
    print "always"
}

$x = 1
$x = 2
print $x

$left = 1
$right = 2
if not ($left == $right) {
    print "differs"
}
"#;

fn main() {
    let report = diagnostics("example.xcl", SOURCE);
    let map = SourceMap::new("example.xcl", SOURCE);

    println!("=== human ===\n");
    print!("{}", report.render_human(&map));

    println!("\n=== json (one object per line) ===\n");
    print!("{}", report.render_json(&map));

    println!("\n=== catalogue ===\n");
    for diagnostic in &report.diagnostics {
        let rule = xcl_toolchain::lint::rule(diagnostic.code).expect("catalogued");
        println!(
            "{} {} [{}] {}",
            rule.code,
            rule.name,
            rule.group.name(),
            rule.summary
        );
    }
}
