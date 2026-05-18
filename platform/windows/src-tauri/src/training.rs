//! Spawn the training pipeline. Mirrors `Sources/TrainingController.swift`.
//!
//! macOS shells out to `tools/auto.sh`; on Windows we shell out to
//! `tools/auto.ps1`. That script doesn't exist yet — see TODO below.

use anyhow::{Context, Result};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

#[derive(serde::Serialize, Clone)]
pub struct TrainingProgress {
    pub epoch: u32,
    pub total: u32,
    pub box_loss: f32,
    pub cls_loss: f32,
}

pub struct TrainingJob {
    child: Arc<Mutex<Option<Child>>>,
}

impl TrainingJob {
    /// Start a training run. Returns immediately; progress streams via
    /// `training-progress` and `training-log` events.
    pub fn start(
        app: AppHandle,
        epochs: u32,
        batch: u32,
        imgsz: u32,
    ) -> Result<Self> {
        // TODO(windows-port): tools/auto.ps1 is not yet ported from tools/auto.sh.
        // Until it lands, the user will see a clear "command not found" log line.
        let repo_root = locate_repo_root()?;
        let script = repo_root.join("tools").join("auto.ps1");

        let mut cmd = Command::new("pwsh");
        cmd.arg("-NoProfile")
            .arg("-ExecutionPolicy").arg("Bypass")
            .arg("-File").arg(&script)
            .arg("--epochs").arg(epochs.to_string())
            .arg("--batch").arg(batch.to_string())
            .arg("--imgsz").arg(imgsz.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "spawn pwsh {} (does the script exist? — auto.ps1 is a TODO)",
                script.display()
            )
        })?;

        let stdout = child.stdout.take().context("no stdout")?;
        let stderr = child.stderr.take().context("no stderr")?;
        let app_out = app.clone();
        let app_err = app.clone();

        thread::spawn(move || {
            let r = BufReader::new(stdout);
            for line in r.lines().flatten() {
                let _ = app_out.emit("training-log", &line);
                if let Some(p) = parse_ultralytics_epoch(&line) {
                    let _ = app_out.emit("training-progress", &p);
                }
            }
        });
        thread::spawn(move || {
            let r = BufReader::new(stderr);
            for line in r.lines().flatten() {
                let _ = app_err.emit("training-log", &format!("[err] {line}"));
            }
        });

        let child = Arc::new(Mutex::new(Some(child)));
        // Watcher: emit done event when the child exits.
        let app_done = app.clone();
        let child_w = child.clone();
        thread::spawn(move || {
            let mut guard = child_w.lock();
            if let Some(ch) = guard.as_mut() {
                let status = ch.wait().ok();
                let _ = app_done.emit(
                    "training-done",
                    serde_json::json!({ "exit_code": status.and_then(|s| s.code()) }),
                );
            }
        });

        Ok(Self { child })
    }

    pub fn cancel(&self) {
        if let Some(mut ch) = self.child.lock().take() {
            let _ = ch.kill();
        }
    }
}

fn locate_repo_root() -> Result<std::path::PathBuf> {
    // `cargo tauri dev` runs us from `platform/windows/src-tauri`. Walk upward
    // for a directory that contains `tools/`. Fall back to CWD.
    let mut here = std::env::current_dir()?;
    for _ in 0..6 {
        if here.join("tools").is_dir() {
            return Ok(here);
        }
        if !here.pop() { break; }
    }
    Ok(std::env::current_dir()?)
}

/// Parse a line like:
///   `Epoch    GPU_mem   box_loss   cls_loss   dfl_loss  Instances    Size`
///   `   1/50    1.23G     1.234     0.567     0.890        12        640`
fn parse_ultralytics_epoch(line: &str) -> Option<TrainingProgress> {
    let trimmed = line.trim_start();
    let mut parts = trimmed.split_whitespace();
    let epoch_token = parts.next()?;
    let (cur, total) = epoch_token.split_once('/')?;
    let cur: u32 = cur.parse().ok()?;
    let total: u32 = total.parse().ok()?;
    let _gpu = parts.next()?; // "1.23G"
    let box_loss: f32 = parts.next()?.parse().ok()?;
    let cls_loss: f32 = parts.next()?.parse().ok()?;
    Some(TrainingProgress { epoch: cur, total, box_loss, cls_loss })
}
