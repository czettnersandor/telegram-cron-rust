# telegram-cron

A lightweight cron-like job scheduler written in Rust that runs as a **systemd user service** and sends notifications via a **Telegram bot** when scripts produce output.

## How it works

1. Reads a YAML config file at startup.
2. Runs scheduled scripts according to their cron expressions.
3. If a script prints **`NOUPDATE`** to stdout → do nothing.
4. If a script prints **anything else** → send that output as a Telegram message.
5. If a script exits with a non-zero code → send an error notification.
6. Watches the config file for changes and **automatically reloads** without restart.

---

## Quick start

### 1. Build

```bash
cargo build --release
# Binary is at: target/release/telegram-cron
```

### 2. Install

```bash
# Install binary
cargo install

# Create config directory
mkdir -p ~/.config/telegram-cron

# Copy and edit the config
cp config.yaml ~/.config/telegram-cron/config.yaml
$EDITOR ~/.config/telegram-cron/config.yaml

# Copy example scripts (optional)
cp -r scripts ~/.config/telegram-cron/
chmod +x ~/.config/telegram-cron/scripts/*.sh
```

### 3. Create a Telegram bot

1. Message [@BotFather](https://t.me/BotFather) on Telegram.
2. Send `/newbot` and follow the prompts.
3. Copy the **bot token** (looks like `123456:ABCdef...`).
4. Start a chat with your new bot, then visit:
   `https://api.telegram.org/bot<TOKEN>/getUpdates`
   to find your **chat ID** (`"id"` inside `"chat"`).

### 4. Edit config

```yaml
telegram:
  bot_token: "123456789:YOUR_BOT_TOKEN_HERE"
  chat_id:   "YOUR_CHAT_ID_HERE"

jobs:
  health_check:
    schedule: "*/5 * * * *"
    script:   "scripts/health_check.sh"
    timeout:  30
    enabled:  true
```

### 5. Install systemd user service

```bash
# Create the systemd user directory if it doesn't exist
mkdir -p ~/.config/systemd/user

# Install the service file
cp telegram-cron.service ~/.config/systemd/user/

# Reload systemd, enable and start
systemctl --user daemon-reload
systemctl --user enable --now telegram-cron

# View logs
journalctl --user -u telegram-cron -f
```

---

## Config reference

```yaml
telegram:
  bot_token: "string"    # Required: Telegram bot token
  chat_id:   "string"    # Required: Telegram chat/user ID

# Optional: override base directory for script paths (default: same dir as config)
scripts_dir: "/home/user/.config/telegram-cron/scripts"

jobs:
  job_name:                     # Unique name shown in Telegram messages
    schedule: "* * * * * *"     # Standard 5-field cron expression (see below)
    script:   "path/to/script"  # Relative to scripts_dir, or absolute
    timeout:  60                # Seconds before the job is considered timed out (default: 60)
    enabled:  true              # Set false to disable without removing (default: true)
    args:                       # Optional list of CLI arguments passed to the script
      - "--flag"
      - "value"
    env:                        # Optional extra environment variables
      KEY: "value"
```

### Cron expression format

```
┌─────────────── second      (0–59)
| ┌───────────── minute      (0–59)
│ │ ┌─────────── hour        (0–23)
│ │ │ ┌───────── day of month (1–31)
│ │ │ │ ┌─────── month        (1–12)
│ │ │ │ ┌───── day of week  (0–7, 0=Sun, 7=Sun)
| │ │ │ │ │
* * * * * *
```

| Expression        | Meaning                  |
|-------------------|--------------------------|
| `0 * * * * *`     | Every minute             |
| `0 */5 * * * *`   | Every 5 minutes          |
| `0 0 * * * *`     | Every hour               |
| `0 0 8 * * *`     | Every day at 08:00       |
| `0 0 9 * * 1`     | Every Monday at 09:00    |
| `0 30 4 1 * *`    | 1st of every month 04:30 |

---

## Writing scripts

Scripts can be **any executable** (shell, Python, etc.).

### Return conventions

| stdout contains | Action                          |
|-----------------|---------------------------------|
| `NOUPDATE`      | Do nothing (success, no change) |
| Any other text  | Send text as Telegram message   |
| *(empty)*       | Do nothing                      |

Non-zero exit code → send an error notification regardless of stdout.

### Passing arguments

```yaml
jobs:
  weather_amsterdam:
    schedule: "0 0 8 * * *"
    script: "scripts/weather.py"
    args:
      - "--lat"
      - "52.3676"
      - "--lon"
      - "4.9041"
      - "--city"
      - "Amsterdam"
```

In your script (`weather.py`):
```python
import argparse
parser = argparse.ArgumentParser()
parser.add_argument("--lat", type=float)
parser.add_argument("--lon", type=float)
parser.add_argument("--city")
args = parser.parse_args()
```

### Passing environment variables

```yaml
jobs:
  ssl_check:
    script: "scripts/ssl_check.sh"
    env:
      ALERT_DAYS: "14"
```

In your script (`ssl_check.sh`):
```bash
ALERT_DAYS="${ALERT_DAYS:-14}"
```

---

## Multiple instances of the same script

Run the same script with different parameters by defining multiple jobs:

```yaml
jobs:
  weather_amsterdam:
    schedule: "0 0 8 * * *"
    script: "scripts/weather.py"
    args: ["--lat", "52.37", "--lon", "4.90", "--city", "Amsterdam"]

  weather_berlin:
    schedule: "0 0 8 * * *"
    script: "scripts/weather.py"
    args: ["--lat", "52.52", "--lon", "13.40", "--city", "Berlin"]
```

---

## Config hot-reload

Edit `~/.config/telegram-cron/config.yaml` and save. The service detects the change within ~5 seconds and automatically restarts the scheduler with the new config — **no `systemctl restart` needed**.

---

## Logs

```bash
# Follow live logs
journalctl --user -u telegram-cron -f

# Last 100 lines
journalctl --user -u telegram-cron -n 100

# Logs since yesterday
journalctl --user -u telegram-cron --since yesterday
```

Set `RUST_LOG=debug` in the service file for verbose output.

---

## Run manually (without systemd)

```bash
telegram-cron /path/to/config.yaml

# Or using the default path (~/.config/telegram-cron/config.yaml)
telegram-cron
```
