//! 子进程生命周期、输出上限和取消；不读取 GalGear 配置或凭据。
use super::Progress;
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

struct Worker(std::process::Child);
impl Drop for Worker {
    fn drop(&mut self) {
        // 任何协议或 I/O 错误都不能遗留后台训练；worker 的 Windows Job 清理后代。
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// 启动固定工作脚本，输入通过 stdin JSON 传递，取消时终止整个进程树。
pub fn run<I: serde::Serialize, R: serde::de::DeserializeOwned>(
    runtime: &Path,
    worker: &Path,
    action: &str,
    input: &I,
    cancelled: &AtomicBool,
    progress: impl Fn(Progress),
) -> Result<R, String> {
    let config: serde_json::Value = serde_json::from_slice(
        &std::fs::read(runtime).map_err(|e| format!("voice.runtime_missing: {e}"))?,
    )
    .map_err(|e| e.to_string())?;
    let python = config["python"].as_str().ok_or("voice.runtime_missing")?;
    let mut command = Command::new(python);
    command
        .args(["-B", "-u"])
        .arg(worker)
        .arg(runtime)
        .arg(action)
        .arg(std::process::id().to_string())
        .env("PYTHONUTF8", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut owned = Worker(command.spawn().map_err(|e| e.to_string())?);
    let child = &mut owned.0;
    let request = serde_json::to_vec(input).map_err(|e| e.to_string())?;
    let written = child
        .stdin
        .take()
        .ok_or("Missing worker stdin")
        .and_then(|mut stream| {
            stream
                .write_all(&request)
                .map_err(|_| "Cannot write worker input")
        });
    if let Err(e) = written {
        let _ = child.kill();
        let _ = child.wait();
        return Err(e.into());
    }
    let stdout = child.stdout.take().ok_or("Missing worker stdout")?;
    let stderr = child.stderr.take().ok_or("Missing worker stderr")?;
    let (sender, receiver) = mpsc::sync_channel(64);
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout.take(8 * 1024 * 1024)).lines() {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let errors = std::thread::spawn(move || {
        let mut tail = String::new();
        // 持续排空 stderr，避免管道反压阻塞训练；只保留最后的诊断。
        for line in BufReader::new(stderr).lines() {
            match line {
                Ok(line) => {
                    tail.push_str(&line);
                    tail.push('\n');
                    if tail.len() > 8192 {
                        tail = tail
                            .chars()
                            .rev()
                            .take(2048)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect();
                    }
                }
                Err(_) => break,
            }
        }
        tail
    });
    let started = Instant::now();
    let mut reply = None;
    let mut worker_error = None;
    let status = loop {
        let output_finished = reader.is_finished();
        while let Ok(line) = receiver.try_recv() {
            let line = line.map_err(|e| e.to_string())?;
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                match value["event"].as_str() {
                    Some("progress") => {
                        if let Ok(p) = serde_json::from_value(value["data"].clone()) {
                            progress(p);
                        }
                    }
                    Some("result") => {
                        reply = Some(
                            serde_json::from_value::<R>(value["data"].clone())
                                .map_err(|e| e.to_string())?,
                        )
                    }
                    Some("error") => {
                        worker_error = Some(
                            value["detail"]
                                .as_str()
                                .unwrap_or("voice.failed")
                                .to_owned(),
                        )
                    }
                    _ => {}
                }
            }
        }
        let timeout = if action == "speak" { 600 } else { 48 * 3600 };
        if cancelled.load(Ordering::Relaxed) || started.elapsed() > Duration::from_secs(timeout) {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let _ = Command::new("taskkill.exe")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .creation_flags(0x08000000)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = child.kill();
            let _ = child.wait();
            drop(receiver);
            let _ = reader.join();
            let _ = errors.join();
            return Err(if cancelled.load(Ordering::Relaxed) {
                "voice.cancelled"
            } else {
                "voice.timeout"
            }
            .into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            // 输出线程可能还在处理进程退出前的最后一条结果。
            if output_finished {
                break status;
            }
        }
        std::thread::sleep(Duration::from_millis(80));
    };
    let _ = reader.join();
    let diagnostic = errors.join().unwrap_or_default();
    if let Some(error) = worker_error {
        return Err(error);
    }
    if !status.success() {
        return Err(format!("voice.failed: {diagnostic}"));
    }
    reply.ok_or_else(|| format!("voice.failed: missing verified result. {diagnostic}"))
}
