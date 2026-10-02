//! Foreground training process shared by the Windows and Linux shells.
use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub struct TrainingJob { child: Arc<Mutex<Option<Child>>> }
impl TrainingJob {
    pub fn is_running(&self) -> bool { self.child.lock().is_some() }
    pub fn start(app: AppHandle, epochs: u32, batch: u32, imgsz: u32, dataset: Option<String>) -> Result<Self> {
        if !(1..=1000).contains(&epochs) || !(1..=128).contains(&batch) || !(320..=2048).contains(&imgsz) || imgsz % 32 != 0 {
            return Err(anyhow!("Epochs and batch must be positive; image size must be a multiple of 32"));
        }
        let repo = locate_repo()?;
        let python = repo.join(if cfg!(windows) { "tools/.venv/Scripts/python.exe" } else { "tools/.venv/bin/python" });
        if !python.exists() { return Err(anyhow!("Training environment missing. Run tools/setup_env.sh (Linux) or tools/setup_env.ps1 (Windows) in {}", repo.display())); }
        let mut command = Command::new(python);
        if let Some(path) = dataset.filter(|p| !p.trim().is_empty()) {
            let path = std::path::PathBuf::from(path);
            if !path.is_file() || !matches!(path.extension().and_then(|s| s.to_str()), Some("yaml" | "yml")) {
                return Err(anyhow!("Choose an existing YAML dataset configuration"));
            }
            command.arg(repo.join("tools/train_logos.py")).arg("--data").arg(path).arg("--install");
        } else {
            command.arg(repo.join("tools/train_from_labels.py"));
        }
        command.args(["--epochs", &epochs.to_string(), "--batch", &batch.to_string(), "--imgsz", &imgsz.to_string()])
            .current_dir(&repo).env("PYTHONUNBUFFERED", "1")
            .stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)] {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().context("start training")?;
        for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
            child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>)] {
            let app = app.clone();
            if let Some(pipe) = pipe { thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(std::result::Result::ok) {
                    let _ = app.emit("training-log", line);
                }
            }); }
        }
        let child = Arc::new(Mutex::new(Some(child)));
        let watched = child.clone();
        thread::spawn(move || loop {
            // Never hold the mutex while waiting: cancellation must acquire it.
            let mut guard = watched.lock();
            let Some(process) = guard.as_mut() else { break; };
            let result = process.try_wait();
            match result {
                Ok(Some(status)) => {
                    *guard = None;
                    let _ = app.emit("training-finished", serde_json::json!({ "exitCode": status.code() }));
                    break;
                }
                Err(error) => {
                    *guard = None;
                    let _ = app.emit("training-log", error.to_string());
                    let _ = app.emit("training-finished", serde_json::json!({ "exitCode": -1 }));
                    break;
                }
                Ok(None) => {}
            }
            drop(guard);
            thread::sleep(Duration::from_millis(100));
        });
        Ok(Self { child })
    }
    pub fn cancel(&self) {
        if let Some(child) = self.child.lock().as_mut() {
            #[cfg(windows)] {
                let _ = Command::new("taskkill").args(["/PID", &child.id().to_string(), "/T", "/F"]).status();
            }
            #[cfg(unix)] unsafe { libc::kill(-(child.id() as i32), libc::SIGTERM); }
            let _ = child.kill();
        }
    }
}
impl Drop for TrainingJob { fn drop(&mut self) { self.cancel(); } }
fn locate_repo() -> Result<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("LIVEBLOCK_REPO_ROOT") { return Ok(path.into()); }
    for start in [std::env::current_dir().ok(), std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_owned()))].into_iter().flatten() {
        for dir in start.ancestors() {
            if dir.join("tools/train_from_labels.py").exists() { return Ok(dir.to_owned()); }
        }
    }
    Err(anyhow!("Set LIVEBLOCK_REPO_ROOT to the checkout containing tools/train_from_labels.py"))
}
