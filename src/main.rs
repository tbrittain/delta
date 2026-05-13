use anyhow::Result;
use clap::Parser;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;

// bail is only used in the Unix terminal-emulator search path.
#[cfg(not(target_os = "windows"))]
use anyhow::bail;

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

    /// Internal: this instance was spawned by a parent delta to run in a new
    /// terminal window. Skips TTY detection to prevent recursive spawning.
    #[arg(long, hide = true)]
    spawned: bool,

    /// Write a debug log to delta-debug.log in the current directory.
    /// Use this to diagnose issues such as empty diff output.
    #[arg(long)]
    debug: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.debug {
        use simplelog::{Config, LevelFilter, WriteLogger};
        let log_file = std::fs::File::create("delta-debug.log")
            .expect("failed to create delta-debug.log");
        WriteLogger::init(LevelFilter::Debug, Config::default(), log_file)
            .expect("failed to initialise logger");

        log::debug!("[delta] version={}", env!("CARGO_PKG_VERSION"));
        log::debug!("[delta] os={} arch={}", std::env::consts::OS, std::env::consts::ARCH);
        log::debug!(
            "[delta] cwd={}",
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "<error>".into())
        );
        log::debug!("[delta] from={:?} to={:?}", args.from, args.to);
        log::debug!("[delta] spawned={}", args.spawned);
        log::debug!("[delta] is_terminal={}", std::io::stdin().is_terminal());
    }

    // --spawned means we were deliberately launched in a new window by a parent
    // delta process — always run the TUI regardless of TTY state.
    if args.spawned || std::io::stdin().is_terminal() {
        return run_tui(&args);
    }

    run_in_spawned_terminal(&args)
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

    let temp_path = std::env::temp_dir()
        .join(format!("delta-review-{}.md", std::process::id()));

    // Pass --spawned so the inner instance always runs the TUI and never
    // tries to spawn another window, regardless of its TTY state.
    let mut inner_args: Vec<String> = vec![
        args.from.clone(),
        args.to.clone(),
        "--output".to_string(),
        temp_path.to_string_lossy().into_owned(),
        "--spawned".to_string(),
    ];
    if args.json {
        inner_args.push("--json".to_string());
    }
    if args.debug {
        inner_args.push("--debug".to_string());
    }

    spawn_and_wait(&exe, &inner_args)?;

    let content = std::fs::read_to_string(&temp_path).unwrap_or_default();
    let _ = std::fs::remove_file(&temp_path);

    if content.is_empty() {
        return Ok(());
    }

    match &args.output {
        Some(path) => std::fs::write(path, &content)?,
        None => print!("{}", content),
    }

    Ok(())
}

fn spawn_and_wait(exe: &PathBuf, args: &[String]) -> Result<()> {
    #[cfg(target_os = "windows")]
    return spawn_and_wait_windows(exe, args);

    #[cfg(not(target_os = "windows"))]
    return spawn_and_wait_unix(exe, args);
}

/// Windows: use `cmd.exe /c start /wait` to open a new console window.
///
/// CREATE_NEW_CONSOLE was tried first but doesn't work: it creates the window
/// but the child inherits the parent's stdin (a pipe), so is_terminal() still
/// returns false and the child tries to spawn yet another window — infinite
/// recursion. cmd.exe's `start` command creates a fresh process with its own
/// console stdin not connected to the parent's pipe chain, so is_terminal()
/// returns true in the inner delta. We also run cmd.exe itself with
/// CREATE_NO_WINDOW so only the delta window is visible, not a cmd window.
#[cfg(target_os = "windows")]
fn spawn_and_wait_windows(exe: &PathBuf, args: &[String]) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let exe_str = exe.to_string_lossy();

    // Quote a token for cmd.exe: wrap in double quotes, escape internal quotes.
    let quote = |s: &str| -> String {
        format!("\"{}\"", s.replace('"', "\"\""))
    };

    // Empty title ("") is required — a non-empty title causes cmd.exe to
    // treat it as the program name rather than the window title.
    let mut cmd_str = format!("start \"\" /wait {}", quote(&exe_str));
    for arg in args {
        cmd_str.push(' ');
        if arg.contains(' ') || arg.contains('"') || arg.is_empty() {
            cmd_str.push_str(&quote(arg));
        } else {
            cmd_str.push_str(arg);
        }
    }

    Command::new("cmd.exe")
        .args(["/c", &cmd_str])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to open console window: {e}"))?
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
