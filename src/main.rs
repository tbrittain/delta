use anyhow::{bail, Result};
use clap::Parser;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;

use delta::export;
use delta::git::{GitBackend, SystemGit};
use delta::ui;

#[derive(Parser, Debug)]
#[command(
    name = "delta",
    about = "Terminal diff review tool for AI-assisted development workflows"
)]
struct Args {
    /// Start ref — the older end of the range (e.g. main, HEAD^, abc1234). Defaults to HEAD~
    #[arg(default_value = "HEAD~")]
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

    if !std::io::stdin().is_terminal() {
        return run_in_spawned_terminal(&args);
    }

    run_tui(&args)
}

fn run_tui(args: &Args) -> Result<()> {
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

    match &args.output {
        Some(path) => std::fs::write(path, &output)?,
        None => print!("{}", output),
    }

    Ok(())
}

/// Called when delta is invoked without a TTY (e.g. from a Claude Code `!` command).
/// Spawns a new terminal window running delta interactively, waits for it to close,
/// then reads the output and prints it to stdout so the caller captures it.
fn run_in_spawned_terminal(args: &Args) -> Result<()> {
    let exe = std::env::current_exe()?;

    // Write to a temp file; the inner instance handles its own --output if set
    let temp_path = std::env::temp_dir()
        .join(format!("delta-review-{}.md", std::process::id()));

    // Build the inner delta invocation: same refs, always write to tempfile
    let mut inner_args: Vec<String> = vec![
        args.from.clone(),
        args.to.clone(),
        "--output".to_string(),
        temp_path.to_string_lossy().into_owned(),
    ];
    if args.json {
        inner_args.push("--json".to_string());
    }

    spawn_and_wait(&exe, &inner_args)?;

    // Read whatever the inner instance wrote
    let content = std::fs::read_to_string(&temp_path).unwrap_or_default();
    let _ = std::fs::remove_file(&temp_path);

    if content.is_empty() {
        return Ok(());
    }

    // If the caller specified --output, honour it; otherwise print to stdout
    match &args.output {
        Some(path) => std::fs::write(path, &content)?,
        None => print!("{}", content),
    }

    Ok(())
}

/// Spawn delta in a new visible terminal window and wait for it to exit.
/// Platform-specific: Windows uses CREATE_NEW_CONSOLE; Unix tries common emulators.
fn spawn_and_wait(exe: &PathBuf, args: &[String]) -> Result<()> {
    #[cfg(target_os = "windows")]
    return spawn_and_wait_windows(exe, args);

    #[cfg(not(target_os = "windows"))]
    return spawn_and_wait_unix(exe, args);
}

/// Windows: spawn with CREATE_NEW_CONSOLE so the OS opens a new visible console window.
/// The new process inherits the environment (PATH, cwd) from the parent.
#[cfg(target_os = "windows")]
fn spawn_and_wait_windows(exe: &PathBuf, args: &[String]) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x00000010;

    Command::new(exe)
        .args(args)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .context("Failed to open a new console window. Please run delta directly in a terminal.")?
        .wait()?;

    Ok(())
}

/// Unix: try $TERMINAL then common terminal emulators in order, skip any not installed.
#[cfg(not(target_os = "windows"))]
fn spawn_and_wait_unix(exe: &PathBuf, args: &[String]) -> Result<()> {
    let mut candidates: Vec<(String, Vec<&str>)> = Vec::new();

    if let Ok(term) = std::env::var("TERMINAL") {
        candidates.push((term, vec!["-e"]));
    }

    candidates.extend([
        ("xterm".into(),           vec!["-e"]),
        ("kitty".into(),           vec!["--"]),
        ("alacritty".into(),       vec!["-e"]),
        ("gnome-terminal".into(),  vec!["--wait", "--"]),
        ("konsole".into(),         vec!["-e"]),
        ("xfce4-terminal".into(),  vec!["--disable-server", "-e"]),
    ]);

    for (term, flags) in &candidates {
        let mut cmd = Command::new(term);
        cmd.args(flags).arg(exe).args(args);
        match cmd.spawn() {
            Ok(mut child) => {
                child.wait()?;
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => bail!("Failed to launch {}: {}", term, e),
        }
    }

    bail!(
        "No terminal emulator found. Please run delta directly in a terminal, \
        or set $TERMINAL to your preferred terminal emulator (e.g. export TERMINAL=xterm)."
    )
}
