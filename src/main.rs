use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

mod app;
mod diff;
mod export;
mod git;
mod ui;

#[derive(Parser, Debug)]
#[command(
    name = "delta",
    about = "Terminal diff review tool for AI-assisted development workflows"
)]
struct Args {
    /// Base ref to diff against (e.g. main, origin/main)
    base: String,

    /// Write output to a file instead of stdout
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Export as JSON instead of markdown
    #[arg(long)]
    json: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let files = git::changed_files(&args.base)?;

    if files.is_empty() {
        eprintln!("No changes found between HEAD and {}", args.base);
        return Ok(());
    }

    let notes = ui::run(files, &args.base)?;

    if notes.is_empty() {
        return Ok(());
    }

    let output = if args.json {
        export::to_json(&notes)?
    } else {
        export::to_markdown(&notes)
    };

    match args.output {
        Some(path) => std::fs::write(&path, &output)?,
        None => print!("{}", output),
    }

    Ok(())
}
