use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

use delta::export;
use delta::git::{GitBackend, SystemGit};
use delta::ui;

#[derive(Parser, Debug)]
#[command(
    name = "delta",
    about = "Terminal diff review tool for AI-assisted development workflows"
)]
struct Args {
    /// Start ref — the older end of the range (e.g. main, HEAD^, abc1234)
    from: String,

    /// End ref — the newer end of the range (defaults to HEAD)
    #[arg(default_value = "HEAD")]
    to: String,

    /// Write output to a file instead of stdout
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Export as JSON instead of markdown
    #[arg(long)]
    json: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let git = SystemGit::new();

    let files = git.changed_files(&args.from, &args.to)?;

    if files.is_empty() {
        eprintln!("No changes found between {} and {}", args.from, args.to);
        return Ok(());
    }

    let notes = ui::run(files, &args.from, &args.to, &git)?;

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
