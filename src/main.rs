mod config;
mod job;
mod scheduler;
mod telegram;

use config::{get_scripts_base, load_config};
use inotify::{Inotify, WatchMask};
use log::{error, info, warn};
use scheduler::run_scheduler;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use telegram::{run_bot_listener, TelegramClient};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

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

    // Clear bot commands on first start
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
        let client = TelegramClient::new(
            config.telegram.bot_token.clone(),
            config.telegram.chat_id.clone(),
        );

        if first_start {
            if let Err(e) = client.clear_commands() {
                warn!("Failed to clear bot commands: {}", e);
            }
            first_start = false;
        }

        let job_names: Vec<String> = config.jobs.keys().cloned().collect();
        if let Err(e) = client.set_commands(&job_names) {
            warn!("Failed to set bot commands: {}", e);
        }

        // ── Start scheduler and bot listener threads ──────────────────────────
        let stop = Arc::new(AtomicBool::new(false));
        let stop_sched = Arc::clone(&stop);
        let stop_bot = Arc::clone(&stop);

        let config_sched = config.clone();
        let base_sched = scripts_base.clone();
        let config_bot = config.clone();
        let base_bot = scripts_base.clone();

        let sched_thread = thread::spawn(move || {
            run_scheduler(config_sched, base_sched, stop_sched);
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
        let mut inotify =
            Inotify::init().expect("Failed to initialize inotify — are you on Linux?");

        // The watch must be added after the file exists; if it doesn't yet,
        // fall back to a short sleep-and-retry so we don't crash on startup.
        loop {
            match inotify
                .watches()
                .add(&config_path, WatchMask::CLOSE_WRITE | WatchMask::MOVED_TO)
            {
                Ok(_) => break,
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
            Ok(_) => {
                info!("Config changed (inotify), debouncing...");
                // Debounce to ensure file write is complete
                thread::sleep(Duration::from_millis(200));
                info!("Reloading config...");
            }
            Err(e) => error!("inotify read error: {}", e),
        }

        // Signal both threads to stop, then wait for them to exit cleanly.
        stop.store(true, Ordering::Relaxed);

        if let Err(e) = sched_thread.join() {
            error!("Scheduler thread panicked: {:?}", e);
        }
        if let Err(e) = bot_thread.join() {
            error!("Bot listener thread panicked: {:?}", e);
        }

        info!("Restarting with new config…");
    }
}
