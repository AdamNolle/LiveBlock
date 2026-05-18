//! Spawns the same `tools/auto.sh` pipeline the macOS app uses, streams its
//! log to the frontend.
//!
//! On Linux the env is identical to macOS: shell, Python venv, ultralytics,
//! coremltools (we replace the CoreML export with an ONNX export for our
//! ort runtime).

use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{Receiver, Sender};
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub enum TrainEvent {
    Stdout(String),
    Stderr(String),
    Finished { exit_code: i32 },
}

pub fn start_training(repo_root: PathBuf, data_yaml: PathBuf) -> Result<Receiver<TrainEvent>> {
    let auto_sh = repo_root.join("tools").join("auto.sh");
    if !auto_sh.exists() {
        return Err(anyhow!("missing {}", auto_sh.display()));
    }

    let mut child = Command::new("bash")
        .arg(auto_sh)
        .arg(data_yaml)
        .arg("--epochs")
        .arg("50")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn auto.sh")?;

    let (tx, rx) = crossbeam_channel::unbounded();
    let stdout = child.stdout.take().context("stdout pipe")?;
    let stderr = child.stderr.take().context("stderr pipe")?;

    let tx_out = tx.clone();
    std::thread::spawn(move || pump(stdout, tx_out, true));
    let tx_err = tx.clone();
    std::thread::spawn(move || pump(stderr, tx_err, false));

    std::thread::spawn(move || {
        let exit = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
        let _ = tx.send(TrainEvent::Finished { exit_code: exit });
    });

    Ok(rx)
}

fn pump<R: std::io::Read + Send + 'static>(reader: R, tx: Sender<TrainEvent>, is_stdout: bool) {
    use std::io::{BufRead, BufReader};
    let buf = BufReader::new(reader);
    for line in buf.lines() {
        let Ok(line) = line else { continue };
        let evt = if is_stdout {
            TrainEvent::Stdout(line)
        } else {
            TrainEvent::Stderr(line)
        };
        if tx.send(evt).is_err() {
            break;
        }
    }
}
