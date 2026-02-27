use crate::config::{JobConfig, TelegramConfig};
use crate::telegram::client::TelegramClient;
use log::{error, info};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

/// Run a job script and send notifications based on output.
pub fn run_job(
    job_name: &str,
    job: &JobConfig,
    scripts_base: &Path,
    telegram: &TelegramConfig,
    manual_trigger: bool,
) {
    info!("[{}] Running", job_name);

    let script_path = if Path::new(&job.script).is_absolute() {
        PathBuf::from(&job.script)
    } else {
        scripts_base.join(&job.script)
    };

    if !script_path.exists() {
        let msg = format!(
            "⚠️ <b>telegram-cron</b>\nJob: <code>{}</code>\nScript not found: <code>{}</code>",
            job_name,
            script_path.display()
        );
        error!("[{}] Script not found: {}", job_name, script_path.display());
        notify(telegram, job_name, &msg);
        return;
    }

    let mut cmd = Command::new(&script_path);
    cmd.args(&job.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (k, v) in &job.env {
        cmd.env(k, v);
    }

    // Spawn the process
    let mut child = match cmd.spawn() {
        Err(e) => {
            let msg = format!(
                "⚠️ <b>telegram-cron</b>\nJob: <code>{}</code>\nFailed to spawn: {}",
                job_name, e
            );
            error!("[{}] Failed to spawn: {}", job_name, e);
            notify(telegram, job_name, &msg);
            return;
        }
        Ok(child) => child,
    };

    // Wait with timeout
    let timeout_duration = Duration::from_secs(job.timeout);
    let output = match child.wait_timeout(timeout_duration) {
        Ok(Some(_status)) => {
            // Process exited within timeout - get full output
            match child.wait_with_output() {
                Ok(output) => output,
                Err(e) => {
                    error!("[{}] Failed to read output: {}", job_name, e);
                    let msg = format!(
                        "⚠️ <b>telegram-cron</b>\nJob: <code>{}</code>\nFailed to read output: {}",
                        job_name, e
                    );
                    notify(telegram, job_name, &msg);
                    return;
                }
            }
        }
        Ok(None) => {
            // Timeout - kill the process
            error!("[{}] Timeout after {} seconds", job_name, job.timeout);
            let _ = child.kill();
            let _ = child.wait(); // Reap zombie

            let msg = format!(
                "⏱️ <b>telegram-cron</b>\nJob: <code>{}</code>\nTimeout after {} seconds",
                job_name, job.timeout
            );
            notify(telegram, job_name, &msg);
            return;
        }
        Err(e) => {
            error!("[{}] Wait error: {}", job_name, e);
            let _ = child.kill();

            let msg = format!(
                "⚠️ <b>telegram-cron</b>\nJob: <code>{}</code>\nWait error: {}",
                job_name, e
            );
            notify(telegram, job_name, &msg);
            return;
        }
    };

    // Process the output

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        let detail = if !stderr.is_empty() {
            format!("Stderr:\n<pre>{}</pre>", truncate(&stderr, 3000))
        } else if !stdout.is_empty() {
            format!("Stdout:\n<pre>{}</pre>", truncate(&stdout, 3000))
        } else {
            "(no output)".to_string()
        };

        let msg = format!(
            "❌ <b>telegram-cron</b>\nJob: <code>{}</code>\nExit code: {}\n{}",
            job_name,
            output.status.code().unwrap_or(-1),
            detail
        );
        error!(
            "[{}] Script failed (exit {:?})",
            job_name,
            output.status.code()
        );
        notify(telegram, job_name, &msg);
        return;
    }

    // Handle successful execution
    match stdout.as_str() {
        "NOUPDATE" => {
            info!("[{}] NOUPDATE — no notification sent", job_name);
            if manual_trigger {
                let msg = format!("💬 <b>{}</b>\nScript had nothing to say", job_name);
                notify(telegram, job_name, &msg);
            }
        }
        "" => {
            info!("[{}] Empty output — no notification sent", job_name);
        }
        text => {
            info!("[{}] Sending: {}", job_name, truncate(text, 80));
            let msg = format!("🔔 <b>{}</b>\n{}", job_name, truncate(text, 4000));
            notify(telegram, job_name, &msg);
        }
    }
}

/// Send a Telegram message, logging any error without panicking.
fn notify(telegram: &TelegramConfig, job_name: &str, msg: &str) {
    let client = TelegramClient::new(telegram.bot_token.clone(), telegram.chat_id.clone());

    if let Err(e) = client.send_message(msg) {
        error!("[{}] Telegram send failed: {}", job_name, e);
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let t: String = s.chars().take(max_chars).collect();
        format!("{}…", t)
    }
}
