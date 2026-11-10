//! `cargo xtask drive`: starts a driven test run of the app (docs/DRIVE.md) and talks to it.
//!
//!   cargo xtask drive start [--fresh] [--visible] [--gpu]
//!   cargo xtask drive <command> [key=value ...]     e.g. `drive click x=40 y=120`, `drive shot`
//!   cargo xtask drive stop
//!
//! The run's folder is `target/drive`: `in.jsonl` and `out.jsonl` (the commands and their
//! answers), screenshots, `app.log`, and the profile the app keeps its settings and sessions in.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::{Result, root, run, target_dir};

const IN: &str = "in.jsonl";
const OUT: &str = "out.jsonl";

fn dir() -> PathBuf {
    root().join("target").join("drive")
}

pub fn drive(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("start") => start(&args[1..]),
        Some(name) => {
            let cmd = command(name, &args[1..])?;
            let patience = Duration::from_secs(15) + Duration::from_millis(cmd["ms"].as_u64().unwrap_or(0));
            let answer = send(cmd, patience)?;
            println!("{answer}");
            if answer["ok"] == true { Ok(()) } else { Err("the command failed".into()) }
        }
        None => {
            Err("usage: cargo xtask drive start [--fresh] [--visible] [--gpu] | <command> [key=value ...] | stop"
                .into())
        }
    }
}

/// Builds the app with the `drive` feature and starts it on `target/drive`, out of sight.
fn start(flags: &[String]) -> Result<()> {
    let flag = |name: &str| flags.iter().any(|f| f == name);
    let dir = dir();
    if dir.join(OUT).exists() && send(command("wait", &["ms=0".into()])?, Duration::from_secs(2)).is_ok() {
        return Err("a driven run is already going; `cargo xtask drive stop` ends it".into());
    }
    if flag("--fresh") {
        let _ = std::fs::remove_dir_all(dir.join("profile"));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for file in [IN, OUT] {
        let _ = std::fs::remove_file(dir.join(file));
    }
    // Partial (grey) text only runs on the GPU, which is Metal on a Mac.
    let gpu = if cfg!(target_os = "macos") { "drive,gpu-metal" } else { "drive,gpu-vulkan" };
    let features = if flag("--gpu") { gpu } else { "drive" };
    run(Command::new(env!("CARGO")).args(["build", "-p", "chidori", "--features", features]).current_dir(root()))?;
    let exe = target_dir().join("debug").join(if cfg!(windows) { "chidori.exe" } else { "chidori" });
    let log = std::fs::File::create(dir.join("app.log")).map_err(|e| format!("app.log: {e}"))?;
    let mut app = Command::new(&exe);
    app.arg("--drive").arg(&dir);
    if flag("--visible") {
        app.arg("--drive-visible");
    }
    keep_output_here();
    // Its own output, so the run outlives this command and whatever shell started it.
    app.stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| format!("{}: {e}", exe.display()))?;
    let ready = wait_for(&dir, 0, Duration::from_secs(60))?;
    println!("{ready}");
    Ok(())
}

/// Windows hands a child every inheritable handle, this command's own output among them, and a
/// shell waiting for that output to close (`cargo xtask drive start | ...`) would wait for the
/// app to quit.
fn keep_output_here() {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        unsafe extern "system" {
            fn SetHandleInformation(handle: *mut std::ffi::c_void, mask: u32, flags: u32) -> i32;
        }
        const HANDLE_FLAG_INHERIT: u32 = 1;
        let handles =
            [std::io::stdin().as_raw_handle(), std::io::stdout().as_raw_handle(), std::io::stderr().as_raw_handle()];
        for handle in handles {
            // SAFETY: a call on this process's own standard handles; a failure changes nothing.
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
        }
    }
}

/// `click x=40 y=120` as `{"do":"click","x":40,"y":120}`: a value that reads as JSON is JSON,
/// anything else a string. (A shell-friendly form: PowerShell 5.1 strips the quotes from JSON
/// passed to a program.)
fn command(name: &str, pairs: &[String]) -> Result<Value> {
    let mut cmd = Map::new();
    cmd.insert("do".into(), if name == "stop" { "quit".into() } else { name.into() });
    for pair in pairs {
        let (key, value) = pair.split_once('=').ok_or_else(|| format!("{pair}: expected key=value"))?;
        cmd.insert(key.into(), serde_json::from_str(value).unwrap_or_else(|_| value.into()));
    }
    Ok(Value::Object(cmd))
}

/// Appends a command and waits for its answer: the app numbers them by their line.
fn send(cmd: Value, patience: Duration) -> Result<Value> {
    let dir = dir();
    let path = dir.join(IN);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(file, "{cmd}").map_err(|e| e.to_string())?;
    drop(file);
    let sent = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let n = sent.lines().filter(|l| !l.trim().is_empty()).count() as u64;
    wait_for(&dir, n, patience)
}

fn wait_for(dir: &Path, n: u64, patience: Duration) -> Result<Value> {
    let until = Instant::now() + patience;
    loop {
        let text = std::fs::read_to_string(dir.join(OUT)).unwrap_or_default();
        let answer = text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find(|v| v["n"] == n);
        if let Some(answer) = answer {
            return Ok(answer);
        }
        if Instant::now() > until {
            return Err(format!(
                "no answer in {} s; is a driven run going? (see {})",
                patience.as_secs(),
                dir.join("app.log").display()
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
