use anyhow::Result;
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;

mod collect;
mod highlight;
mod model;
mod render;
mod serve;

#[derive(Parser)]
#[command(name = "xcl-doc", version, about = "Documentation generator for XCL")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build the HTML documentation.
    Build {
        /// Output directory.
        #[arg(short, long, default_value = "target/xcl-docs")]
        out: PathBuf,

        /// Optional path to a user .xcl file or directory to include.
        #[arg(short, long)]
        src: Option<PathBuf>,
    },

    /// Serve the documentation on a local port.
    Serve {
        #[arg(short, long, default_value = "target/xcl-docs")]
        dir: PathBuf,

        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },

    /// Print the search index to stdout.
    Index,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
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

    // Domains — one page each
    for d in &model.domains {
        let sub = root.join(&d.name);
        fs::create_dir_all(&sub)?;
        fs::write(sub.join("index.html"), render::domain::render(&d.name, model))?;
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
    fs::write(root.join("static").join("search-index.js"),
        render::search::render_js(model))?;

    Ok(())
}
