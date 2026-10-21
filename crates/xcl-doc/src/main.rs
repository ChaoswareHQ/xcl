use anyhow::Result;
use std::fs;
use std::path::PathBuf;

mod collect;
mod highlight;
mod model;
mod render;
mod serve;

enum Cmd {
    Build { out: PathBuf, src: Option<PathBuf> },
    Serve { dir: PathBuf, port: u16 },
    Index,
    Help,
    Version,
}

const USAGE: &str = "
Usage: xcl-doc <COMMAND> [OPTIONS]

Commands:
  build [-o DIR] [-s PATH]   Build the HTML documentation
  serve [-d DIR] [-p PORT]   Serve the documentation on a local port
  index                      Print the search index to stdout

Options:
  -h, --help       Show this help message
  -v, --version    Show version information
";

fn fail(message: &str) -> ! {
    eprintln!("xcl-doc: error: {message}");
    eprintln!("Try 'xcl-doc --help' for usage.");
    std::process::exit(1)
}

fn parse_args<I: IntoIterator<Item = String>>(iter: I) -> Cmd {
    let mut args = iter.into_iter().skip(1);
    let Some(command) = args.next() else {
        fail("missing command");
    };

    match command.as_str() {
        "-h" | "--help" => return Cmd::Help,
        "-v" | "--version" => return Cmd::Version,
        "index" => return Cmd::Index,
        "build" | "serve" => {}
        other => fail(&format!("unknown command '{other}'")),
    }

    let mut out = PathBuf::from("target/xcl-docs");
    let mut src: Option<PathBuf> = None;
    let mut dir = PathBuf::from("target/xcl-docs");
    let mut port: u16 = 8080;

    while let Some(arg) = args.next() {
        let mut value = |flag: &str| -> String {
            args.next()
                .unwrap_or_else(|| fail(&format!("{flag} requires a value")))
        };
        match arg.as_str() {
            "-o" | "--out" => out = PathBuf::from(value("--out")),
            "-s" | "--src" => src = Some(PathBuf::from(value("--src"))),
            "-d" | "--dir" => dir = PathBuf::from(value("--dir")),
            "-p" | "--port" => {
                port = value("--port")
                    .parse()
                    .unwrap_or_else(|_| fail("--port requires a number"));
            }
            other => fail(&format!("unknown option '{other}'")),
        }
    }

    if command == "build" {
        Cmd::Build { out, src }
    } else {
        Cmd::Serve { dir, port }
    }
}

fn main() -> Result<()> {
    let cli = parse_args(std::env::args());

    match cli {
        Cmd::Help => print!("{USAGE}"),
        Cmd::Version => println!("xcl-doc {}", env!("CARGO_PKG_VERSION")),
        Cmd::Build { out, src } => {
            let mut model = collect::collect_std();
            if let Some(path) = src {
                collect::collect_src(&path, &mut model)?;
            }
            write_docs(&out, &model)?;
            println!("Wrote docs to {}", out.display());
        }

        Cmd::Serve { dir, port } => {
            serve::serve(dir, port)?;
        }

        Cmd::Index => {
            let model = collect::collect_std();
            print!("{}", render::search::render_js(&model));
        }
    }
    Ok(())
}

fn write_docs(root: &PathBuf, model: &model::DocModel) -> Result<()> {
    fs::create_dir_all(root)?;
    fs::create_dir_all(root.join("functions"))?;
    fs::create_dir_all(root.join("types"))?;
    fs::create_dir_all(root.join("static"))?;

    // Index
    fs::write(root.join("index.html"), render::index::render(model))?;
    fs::write(root.join("all.html"), render::all::render(model))?;
    fs::write(root.join("tree.html"), render::tree::render(model))?;

    // Domains — one page each
    for d in &model.domains {
        let sub = root.join(&d.name);
        fs::create_dir_all(&sub)?;
        fs::write(
            sub.join("index.html"),
            render::domain::render(&d.name, model),
        )?;
    }

    // Natives — one page each, in functions/
    for n in &model.natives {
        fs::write(
            root.join("functions").join(format!("{}.html", n.full_name)),
            render::native::render(n, model),
        )?;
    }

    // Types — one page each, in types/
    for t in &model.types {
        fs::write(
            root.join("types").join(format!("{}.html", t.name)),
            render::type_::render(t, model),
        )?;
    }

    // Search index — static/search-index.js
    fs::write(
        root.join("static").join("search-index.js"),
        render::search::render_js(model),
    )?;

    Ok(())
}
