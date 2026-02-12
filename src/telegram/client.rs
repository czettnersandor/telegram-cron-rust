use once_cell::sync::Lazy;
use serde::Deserialize;
use serde_json::json;

static HTTP_AGENT: Lazy<ureq::Agent> = Lazy::new(|| {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(35))
        .build()
});

#[derive(Debug, Deserialize)]
pub struct TelegramUpdate {
    pub update_id: i64,
    pub message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
pub struct TelegramMessage {
    pub chat: TelegramChat,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TelegramChat {
    pub id: i64,
}

#[derive(Debug, Deserialize)]
struct TelegramResponse {
    ok: bool,
    result: Option<serde_json::Value>,
}

pub struct TelegramClient {
    token: String,
    pub chat_id: String,
}

impl TelegramClient {
    pub fn new(token: String, chat_id: String) -> Self {
        Self { token, chat_id }
    }

    /// Send a message to the configured chat.
    pub fn send_message(&self, message: &str) -> Result<(), String> {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.token);
        let body = json!({
            "chat_id": &self.chat_id,
            "text": message,
            "parse_mode": "HTML"
        });

        let response = HTTP_AGENT
            .post(&url)
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| format!("HTTP error: {}", e))?;

        if response.status() != 200 {
            return Err(format!("Telegram API returned status {}", response.status()));
        }
        Ok(())
    }

    /// Poll for updates from Telegram.
    pub fn get_updates(&self, offset: i64, timeout: u64) -> Result<Vec<TelegramUpdate>, String> {
        let url = format!(
            "https://api.telegram.org/bot{}/getUpdates?offset={}&timeout={}",
            self.token, offset, timeout
        );

        let response = HTTP_AGENT
            .get(&url)
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

        let result = body.result.ok_or_else(|| "Missing result field".to_string())?;
        let updates: Vec<TelegramUpdate> = serde_json::from_value(result)
            .map_err(|e| format!("Failed to parse updates: {}", e))?;

        Ok(updates)
    }

    /// Clear all bot commands using Telegram's deleteMyCommands API.
    pub fn clear_commands(&self) -> Result<(), String> {
        let url = format!("https://api.telegram.org/bot{}/deleteMyCommands", self.token);

        let response = HTTP_AGENT
            .post(&url)
            .set("Content-Type", "application/json")
            .send_string("{}")
            .map_err(|e| format!("HTTP error: {}", e))?;

        if response.status() != 200 {
            return Err(format!("Telegram API returned status {}", response.status()));
        }

        Ok(())
    }

    /// Set bot commands using Telegram's setMyCommands API.
    pub fn set_commands(&self, job_names: &[String]) -> Result<(), String> {
        let url = format!("https://api.telegram.org/bot{}/setMyCommands", self.token);

        let job_list = if job_names.is_empty() {
            "no jobs configured".to_string()
        } else {
            job_names.join(", ")
        };

        let commands = vec![json!({
            "command": "run",
            "description": format!("Run a job: {}", job_list)
        })];

        let body = json!({
            "commands": commands
        });

        let response = HTTP_AGENT
            .post(&url)
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| format!("HTTP error: {}", e))?;

        if response.status() != 200 {
            return Err(format!("Telegram API returned status {}", response.status()));
        }

        Ok(())
    }
}

/// Escape HTML entities for safe inclusion in Telegram messages.
pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Format a list of jobs for display in Telegram.
pub fn format_job_list(jobs: &[String]) -> String {
    jobs.iter()
        .map(|j| format!("• <code>{}</code>", escape_html(j)))
        .collect::<Vec<_>>()
        .join("\n")
}
