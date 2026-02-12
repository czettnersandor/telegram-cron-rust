use crate::config::AppConfig;
use crate::job::run_job;
use crate::telegram::client::{escape_html, format_job_list, TelegramClient};
use log::{error, info, warn};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use threadpool::ThreadPool;

/// Run the Telegram bot command listener.
pub fn run_bot_listener(
    config: AppConfig,
    scripts_base: PathBuf,
    stop: Arc<AtomicBool>,
) {
    let mut offset: i64 = 0;
    let config = Arc::new(config);
    let client = TelegramClient::new(
        config.telegram.bot_token.clone(),
        config.telegram.chat_id.clone(),
    );

    // Thread pool to prevent unbounded thread spawning
    let pool = ThreadPool::new(4);

    info!("Bot listener started");

    loop {
        if stop.load(Ordering::Relaxed) {
            info!("Bot listener stopping — config reload requested");
            pool.join(); // Wait for pending commands to complete
            return;
        }

        match client.get_updates(offset, 30) {
            Ok(updates) => {
                for update in updates {
                    offset = update.update_id + 1;

                    if let Some(message) = update.message {
                        // Security: Only accept commands from configured chat
                        if message.chat.id.to_string() != client.chat_id {
                            warn!(
                                "Ignoring command from unauthorized chat: {}",
                                message.chat.id
                            );
                            continue;
                        }

                        if let Some(text) = message.text {
                            if text.starts_with('/') {
                                info!("Received command: {} from chat {}", text, message.chat.id);

                                let cfg = config.clone();
                                let base = scripts_base.clone();
                                let chat_id = message.chat.id;
                                let bot_token = config.telegram.bot_token.clone();

                                pool.execute(move || {
                                    handle_command(&text, chat_id, &cfg, &base, &bot_token);
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

/// Handle incoming Telegram commands.
fn handle_command(
    command_text: &str,
    chat_id: i64,
    config: &AppConfig,
    scripts_base: &PathBuf,
    bot_token: &str,
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
                format_job_list(&job_list)
            );
            send_message(bot_token, chat_id, &msg);
            return;
        }

        let job_name = parts[1];
        if let Some(job) = config.jobs.get(job_name) {
            if !job.enabled {
                let msg = format!(
                    "⚠️ Job <code>{}</code> is disabled in config",
                    escape_html(job_name)
                );
                send_message(bot_token, chat_id, &msg);
                return;
            }

            let msg = format!("▶️ Running job <code>{}</code>...", escape_html(job_name));
            send_message(bot_token, chat_id, &msg);

            run_job(
                job_name,
                job,
                scripts_base,
                &config.telegram,
                true,
            );
        } else {
            let job_list: Vec<String> = config.jobs.keys().cloned().collect();
            let msg = format!(
                "❌ Job <code>{}</code> not found.\n\nAvailable jobs:\n{}",
                escape_html(job_name),
                format_job_list(&job_list)
            );
            send_message(bot_token, chat_id, &msg);
        }
    }
}

/// Send a message to a specific chat (helper for command responses).
fn send_message(bot_token: &str, chat_id: i64, message: &str) {
    let client = TelegramClient::new(bot_token.to_string(), chat_id.to_string());
    if let Err(e) = client.send_message(message) {
        error!("Failed to send Telegram message: {}", e);
    }
}
