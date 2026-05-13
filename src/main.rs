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

/// Windows: use CREATE_NEW_CONSOLE to open delta in a new visible console window.
///
/// We pass --spawned so the child always runs the TUI regardless of its own
/// is_terminal() result — this breaks the recursive-spawn loop that made
/// CREATE_NEW_CONSOLE unusable before --spawned existed.
///
/// We avoid cmd.exe /c start entirely: Rust's Command escapes args with \"
/// but cmd.exe only understands "" quoting, so the two conventions conflict
/// and mangle the command line. CreateProcess with CREATE_NEW_CONSOLE is the
/// correct Win32 primitive and requires no shell intermediary.
#[cfg(target_os = "windows")]
fn spawn_and_wait_windows(exe: &PathBuf, args: &[String]) -> Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;

    const CREATE_NEW_CONSOLE: u32 = 0x00000010;

    // current_exe() can return a \\?\ (extended-length) prefixed path on Windows.
    // Strip it so CreateProcess resolves the executable through the normal path.
    let exe_str = exe.to_string_lossy();
    let exe_clean = exe_str.strip_prefix(r"\\?\").unwrap_or(&exe_str);

    Command::new(exe_clean)
        .args(args)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NEW_CONSOLE)
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
