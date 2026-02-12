use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::{Local, Utc};
use cron::Schedule;
use inotify::{Inotify, WatchMask};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::str::FromStr;

// ─── Config structures ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
struct TelegramConfig {
    bot_token: String,
    chat_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct JobConfig {
    schedule: String,
    script: String,
    #[serde(default = "default_timeout")]
    timeout: u64,
    #[serde(default = "default_true")]
    enabled: bool,
    /// CLI arguments forwarded to the script verbatim.
    #[serde(default)]
    args: Vec<String>,
    /// Extra environment variables injected into the script process.
    #[serde(default)]
    env: HashMap<String, String>,
}

fn default_timeout() -> u64 { 60 }
fn default_true() -> bool { true }

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AppConfig {
    telegram: TelegramConfig,
    /// Base directory for scripts.  Relative paths are resolved from the
    /// config file's parent directory.  Defaults to that same parent.
    #[serde(default)]
    scripts_dir: Option<String>,
    jobs: HashMap<String, JobConfig>,
}

// ─── Telegram sender ──────────────────────────────────────────────────────────

fn send_telegram(token: &str, chat_id: &str, message: &str) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
    let body = json!({
        "chat_id": chat_id,
        "text": message,
        "parse_mode": "HTML"
    });

    let response = ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("HTTP error: {}", e))?;

    if response.status() != 200 {
        return Err(format!("Telegram API returned status {}", response.status()));
    }
    Ok(())
}

// ─── Telegram bot commands ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    chat: TelegramChat,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct TelegramResponse {
    ok: bool,
    result: Option<serde_json::Value>,
}

/// Clear all bot commands using Telegram's deleteMyCommands API
fn clear_bot_commands(token: &str) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{}/deleteMyCommands", token);
    
    let response = ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_string("{}")
        .map_err(|e| format!("HTTP error: {}", e))?;

    if response.status() != 200 {
        return Err(format!("Telegram API returned status {}", response.status()));
    }
    
    info!("Bot commands cleared");
    Ok(())
}

/// Set bot commands using Telegram's setMyCommands API
/// This registers commands for autocomplete in Telegram clients
fn set_bot_commands(token: &str, job_names: &[String]) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{}/setMyCommands", token);
    
    // Build description with available job names
    let job_list = if job_names.is_empty() {
        "no jobs configured".to_string()
    } else {
        job_names.join(", ")
    };
    
    let commands = vec![
        json!({
            "command": "run",
            "description": format!("Run a job: {}", job_list)
        }),
    ];
    
    let body = json!({
        "commands": commands
    });

    let response = ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("HTTP error: {}", e))?;

    if response.status() != 200 {
        return Err(format!("Telegram API returned status {}", response.status()));
    }
    
    info!("Bot command registered with {} available jobs", job_names.len());
    Ok(())
}

/// Poll for updates from Telegram
fn get_updates(token: &str, offset: i64, timeout: u64) -> Result<Vec<TelegramUpdate>, String> {
    let url = format!(
        "https://api.telegram.org/bot{}/getUpdates?offset={}&timeout={}",
        token, offset, timeout
    );

    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(timeout + 5))
        .call()
        .map_err(|e| format!("HTTP error: {}", e))?;

    if response.status() != 200 {
        return Err(format!("Telegram API returned status {}", response.status()));
    }

    let body: TelegramResponse = response
        .into_json()
        .map_err(|e| format!("JSON parse error: {}", e))?;

    if !body.ok {
        return Err("Telegram API returned ok=false".to_string());
    }

    let updates: Vec<TelegramUpdate> = serde_json::from_value(body.result.unwrap_or(json!([])))
        .map_err(|e| format!("Failed to parse updates: {}", e))?;

    Ok(updates)
}

/// Handle incoming Telegram commands
fn handle_command(
    command_text: &str,
    chat_id: i64,
    config: &AppConfig,
    scripts_base: &Path,
) {
    let parts: Vec<&str> = command_text.split_whitespace().collect();
    if parts.is_empty() {
        return;
    }

    let command = parts[0].trim_start_matches('/');
    
    // Handle /run job_name
    if command == "run" {
        if parts.len() < 2 {
            let job_list: Vec<String> = config.jobs.keys().cloned().collect();
            let msg = format!(
                "ℹ️ Usage: /run &lt;job_name&gt;\n\nAvailable jobs:\n{}",
                job_list.iter().map(|j| format!("• <code>{}</code>", j)).collect::<Vec<_>>().join("\n")
            );
            let _ = send_telegram(&config.telegram.bot_token, &chat_id.to_string(), &msg);
            return;
        }
        
        let job_name = parts[1];
        if let Some(job) = config.jobs.get(job_name) {
            if !job.enabled {
                let msg = format!("⚠️ Job <code>{}</code> is disabled in config", job_name);
                let _ = send_telegram(&config.telegram.bot_token, &chat_id.to_string(), &msg);
                return;
            }
            
            let msg = format!("▶️ Running job <code>{}</code>...", job_name);
            let _ = send_telegram(&config.telegram.bot_token, &chat_id.to_string(), &msg);
            
            run_job(job_name, job, scripts_base, &config.telegram, true);
        } else {
            let job_list: Vec<String> = config.jobs.keys().cloned().collect();
            let msg = format!(
                "❌ Job <code>{}</code> not found.\n\nAvailable jobs:\n{}",
                job_name,
                job_list.iter().map(|j| format!("• <code>{}</code>", j)).collect::<Vec<_>>().join("\n")
            );
            let _ = send_telegram(&config.telegram.bot_token, &chat_id.to_string(), &msg);
        }
    }
}

/// Run the Telegram bot command listener
fn run_bot_listener(
    config: AppConfig,
    scripts_base: PathBuf,
    stop: Arc<Mutex<bool>>,
) {
    let mut offset: i64 = 0;
    let config = Arc::new(config);
    
    info!("Bot listener started");

    loop {
        if *stop.lock().unwrap() {
            info!("Bot listener stopping — config reload requested");
            return;
        }

        match get_updates(&config.telegram.bot_token, offset, 30) {
            Ok(updates) => {
                for update in updates {
                    offset = update.update_id + 1;
                    
                    if let Some(message) = update.message {
                        if let Some(text) = message.text {
                            if text.starts_with('/') {
                                info!("Received command: {} from chat {}", text, message.chat.id);
                                
                                let cfg = config.clone();
                                let base = scripts_base.clone();
                                let chat_id = message.chat.id;
                                
                                thread::spawn(move || {
                                    handle_command(&text, chat_id, &cfg, &base);
                                });
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to get updates: {}", e);
                thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

// ─── Job runner ───────────────────────────────────────────────────────────────

fn run_job(
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
    cmd.args(&job.args);
    for (k, v) in &job.env {
        cmd.env(k, v);
    }

    match cmd.output() {
        Err(e) => {
            let msg = format!(
                "⚠️ <b>telegram-cron</b>\nJob: <code>{}</code>\nFailed to spawn: {}",
                job_name, e
            );
            error!("[{}] Failed to spawn: {}", job_name, e);
            notify(telegram, job_name, &msg);
        }
        Ok(output) => {
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
                error!("[{}] Script failed (exit {:?})", job_name, output.status.code());
                notify(telegram, job_name, &msg);
                return;
            }

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
    }
}

/// Send a Telegram message, logging any error without panicking.
fn notify(telegram: &TelegramConfig, job_name: &str, msg: &str) {
    if let Err(e) = send_telegram(&telegram.bot_token, &telegram.chat_id, msg) {
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

// ─── Config loader ────────────────────────────────────────────────────────────

fn load_config(path: &Path) -> Result<AppConfig, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
    serde_yaml::from_str(&content)
        .map_err(|e| format!("Cannot parse {}: {}", path.display(), e))
}

fn get_scripts_base(config: &AppConfig, config_path: &Path) -> PathBuf {
    if let Some(dir) = &config.scripts_dir {
        let p = PathBuf::from(dir);
        if p.is_absolute() {
            return p;
        }
        if let Some(parent) = config_path.parent() {
            return parent.join(&p);
        }
        return p;
    }
    config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

// ─── Scheduler ────────────────────────────────────────────────────────────────

struct JobState {
    last_fired: Option<chrono::DateTime<Utc>>,
}

/// Returns true when there is at least one scheduled occurrence in the
/// half-open interval `(window_start, now]`.
/// On the very first tick (no previous fire) we look back one minute so
/// startup doesn't skip an immediately-due job.
fn should_fire(
    schedule: &Schedule,
    last_fired: Option<chrono::DateTime<Utc>>,
    now: chrono::DateTime<Utc>,
) -> bool {
    let window_start = last_fired
        .unwrap_or_else(|| now - chrono::Duration::minutes(1));

    for t in schedule.after(&window_start) {
        return t <= now;
    }
    false
}

fn run_scheduler(
    config: AppConfig,
    scripts_base: PathBuf,
    stop: Arc<Mutex<bool>>,
) {
    let mut job_states: HashMap<String, JobState> = config
        .jobs
        .keys()
        .map(|name| (name.clone(), JobState { last_fired: None }))
        .collect();

    let config = Arc::new(config);
    info!("Scheduler started with {} job(s)", config.jobs.len());

    loop {
        // Honour stop signal (set by the watcher on config change).
        if *stop.lock().unwrap() {
            info!("Scheduler stopping — config reload requested");
            return;
        }

        let now = Utc::now();
        info!("Tick at {}", Local::now().format("%Y-%m-%d %H:%M:%S %Z"));

        for (name, job) in &config.jobs {
            if !job.enabled {
                continue;
            }

            let schedule = match Schedule::from_str(&job.schedule) {
                Ok(s) => s,
                Err(e) => {
                    warn!("[{}] Invalid cron '{}': {}", name, job.schedule, e);
                    continue;
                }
            };

            let state = job_states
                .entry(name.clone())
                .or_insert(JobState { last_fired: None });

            if should_fire(&schedule, state.last_fired, now) {
                state.last_fired = Some(now);

                let job_c  = job.clone();
                let name_c = name.clone();
                let base_c = scripts_base.clone();
                let tg_c   = config.telegram.clone();

                thread::spawn(move || run_job(&name_c, &job_c, &base_c, &tg_c, false));
            }
        }

        thread::sleep(Duration::from_secs(5));
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();

    // Config path: first CLI arg, or the XDG default.
    let config_path: PathBuf = if let Some(arg) = std::env::args().nth(1) {
        PathBuf::from(arg)
    } else {
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").expect("HOME not set")).join(".config")
            });
        base.join("telegram-cron").join("config.yaml")
    };

    info!("Config: {}", config_path.display());

    // ── Clear bot commands on first start ─────────────────────────────────────
    let mut first_start = true;

    loop {
        // ── Load config ───────────────────────────────────────────────────────
        let config = match load_config(&config_path) {
            Ok(c) => c,
            Err(e) => {
                error!("Config error: {}", e);
                info!("Retrying in 10 s…");
                thread::sleep(Duration::from_secs(10));
                continue;
            }
        };

        let scripts_base = get_scripts_base(&config, &config_path);
        info!("Scripts base: {}", scripts_base.display());

        // ── Update bot commands ───────────────────────────────────────────────
        // Clear all commands on first start, then register new ones
        if first_start {
            if let Err(e) = clear_bot_commands(&config.telegram.bot_token) {
                warn!("Failed to clear bot commands: {}", e);
            }
            first_start = false;
        }
        
        let job_names: Vec<String> = config.jobs.keys().cloned().collect();
        if let Err(e) = set_bot_commands(&config.telegram.bot_token, &job_names) {
            warn!("Failed to set bot commands: {}", e);
        }

        // ── Start scheduler thread ────────────────────────────────────────────
        let stop         = Arc::new(Mutex::new(false));
        let stop_sched   = Arc::clone(&stop);
        let stop_bot     = Arc::clone(&stop);
        let config_clone = config.clone();
        let base_clone   = scripts_base.clone();
        let config_bot   = config.clone();
        let base_bot     = scripts_base.clone();

        let sched_thread = thread::spawn(move || {
            run_scheduler(config_clone, base_clone, stop_sched);
        });

        let bot_thread = thread::spawn(move || {
            run_bot_listener(config_bot, base_bot, stop_bot);
        });

        // ── Watch config file with inotify ────────────────────────────────────
        //
        // We re-create the Inotify instance every loop iteration because after
        // a MOVED_TO event (editor atomic-rename) the original inode is gone;
        // the watch would silently stop working if we reused it.
        //
        // WatchMask flags:
        //   CLOSE_WRITE  – file was written and closed in-place (e.g. echo > file)
        //   MOVED_TO     – a temp file was renamed onto our path (vim, nano, etc.)
        let mut inotify = Inotify::init()
            .expect("Failed to initialise inotify — are you on Linux?");

        // The watch must be added after the file exists; if it doesn't yet,
        // fall back to a short sleep-and-retry so we don't crash on startup.
        loop {
            match inotify.watches().add(
                &config_path,
                WatchMask::CLOSE_WRITE | WatchMask::MOVED_TO,
            ) {
                Ok(_)  => break,
                Err(e) => {
                    error!("inotify watch failed ({}), retrying in 2 s…", e);
                    thread::sleep(Duration::from_secs(2));
                }
            }
        }

        info!("Watching {} for changes (inotify)", config_path.display());

        // Block here — zero CPU — until the kernel delivers an event.
        let mut buf = [0u8; 1024];
        match inotify.read_events_blocking(&mut buf) {
            Ok(_)  => info!("Config changed (inotify), reloading…"),
            Err(e) => error!("inotify read error: {}", e),
        }

        // Signal both threads to stop, then wait for them to exit cleanly.
        *stop.lock().unwrap() = true;
        if let Err(e) = sched_thread.join() {
            error!("Scheduler thread panicked: {:?}", e);
        }
        if let Err(e) = bot_thread.join() {
            error!("Bot listener thread panicked: {:?}", e);
        }
        info!("Restarting with new config…");
    }
}
