use crate::config::AppConfig;
use crate::job::run_job;
use chrono::{Local, Utc};
use cron::Schedule;
use log::{info, warn};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

struct JobState {
    last_fired: Option<chrono::DateTime<Utc>>,
}

/// Calculate the next wake time based on all enabled job schedules.
/// Returns the earliest upcoming scheduled time across all jobs.
fn calculate_next_wake_time(
    config: &AppConfig,
    job_states: &HashMap<String, JobState>,
) -> chrono::DateTime<Utc> {
    let now = Utc::now();
    let mut earliest: Option<chrono::DateTime<Utc>> = None;

    for (name, job) in &config.jobs {
        if !job.enabled {
            continue;
        }

        let schedule = match Schedule::from_str(&job.schedule) {
            Ok(s) => s,
            Err(_) => continue, // Skip invalid schedules
        };

        // Get the last time this job fired
        let last_fired = job_states
            .get(name)
            .and_then(|state| state.last_fired);

        // Find the next scheduled time after the last fire (or now if never fired)
        let window_start = last_fired.unwrap_or(now);

        if let Some(next_time) = schedule.after(&window_start).next() {
            earliest = match earliest {
                Some(current_earliest) => Some(current_earliest.min(next_time)),
                None => Some(next_time),
            };
        }
    }

    // If we found a next time, return it; otherwise default to 1 minute from now
    earliest.unwrap_or_else(|| now + chrono::Duration::minutes(1))
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
    let window_start = last_fired.unwrap_or_else(|| now - chrono::Duration::minutes(1));

    for t in schedule.after(&window_start) {
        return t <= now;
    }
    false
}

/// Run the cron scheduler loop.
pub fn run_scheduler(config: AppConfig, scripts_base: PathBuf, stop: Arc<AtomicBool>) {
    let mut job_states: HashMap<String, JobState> = config
        .jobs
        .keys()
        .map(|name| (name.clone(), JobState { last_fired: None }))
        .collect();

    let config = Arc::new(config);
    info!("Scheduler started with {} job(s)", config.jobs.len());

    loop {
        // Honor stop signal (set by the watcher on config change).
        if stop.load(Ordering::Relaxed) {
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

                let job_c = job.clone();
                let name_c = name.clone();
                let base_c = scripts_base.clone();
                let tg_c = config.telegram.clone();

                thread::spawn(move || run_job(&name_c, &job_c, &base_c, &tg_c, false));
            }
        }

        // Calculate the next wake time based on upcoming schedules
        let next_wake = calculate_next_wake_time(&config, &job_states);
        let sleep_duration = (next_wake - now)
            .to_std()
            .unwrap_or(Duration::from_secs(5))
            .min(Duration::from_secs(60)); // Cap at 60 seconds to handle config changes

        info!(
            "Next wake at {} (sleeping for {:?})",
            next_wake.with_timezone(&Local).format("%H:%M:%S"),
            sleep_duration
        );

        thread::sleep(sleep_duration);
    }
}

#[cfg(test)]
mod tests {
    use chrono::Timelike;

    use super::*;
    use crate::config::TelegramConfig;

    #[test]
    fn test_should_fire_first_tick() {
        // Every hour at minute 0
        let schedule = Schedule::from_str("0 0 * * * *").unwrap();
        let now = Utc::now();

        // On first tick with no last_fired, should look back 1 minute
        // This will only fire if we're currently at minute 0
        let fires = should_fire(&schedule, None, now);

        // We can't guarantee what minute it is, so just test the logic works
        assert!(fires == (now.minute() == 0 || now.minute() == 1));
    }

    #[test]
    fn test_should_fire_after_time() {
        let schedule = Schedule::from_str("0 * * * * *").unwrap();

        // If last fired was 2 hours ago, should fire now if we're at minute 0
        let two_hours_ago = Utc::now() - chrono::Duration::hours(2);
        let now = Utc::now();

        // Should have multiple scheduled times between then and now
        let fires = should_fire(&schedule, Some(two_hours_ago), now);
        assert!(fires); // At least one hour boundary crossed
    }

    #[test]
    fn test_calculate_next_wake_time_single_job() {
        use crate::config::JobConfig;
        use std::collections::HashMap;

        let mut jobs = HashMap::new();
        jobs.insert(
            "test_job".to_string(),
            JobConfig {
                schedule: "0 * * * * *".to_string(), // Every hour
                script: "test.sh".to_string(),
                timeout: 60,
                enabled: true,
                args: vec![],
                env: HashMap::new(),
            },
        );

        let config = AppConfig {
            telegram: TelegramConfig {
                bot_token: "test".to_string(),
                chat_id: "test".to_string(),
            },
            scripts_dir: None,
            jobs,
        };

        let job_states = HashMap::new();
        let next_wake = calculate_next_wake_time(&config, &job_states);
        let now = Utc::now();

        // Next wake should be in the future
        assert!(next_wake > now);

        // Should be within the next hour
        assert!(next_wake < now + chrono::Duration::hours(1));
    }

    #[test]
    fn test_calculate_next_wake_time_multiple_jobs() {
        use crate::config::JobConfig;
        use std::collections::HashMap;

        let mut jobs = HashMap::new();

        // Job 1: Every hour
        jobs.insert(
            "hourly".to_string(),
            JobConfig {
                schedule: "0 * * * *".to_string(),
                script: "hourly.sh".to_string(),
                timeout: 60,
                enabled: true,
                args: vec![],
                env: HashMap::new(),
            },
        );

        // Job 2: Every 5 minutes
        jobs.insert(
            "frequent".to_string(),
            JobConfig {
                schedule: "*/5 * * * *".to_string(),
                script: "frequent.sh".to_string(),
                timeout: 60,
                enabled: true,
                args: vec![],
                env: HashMap::new(),
            },
        );

        let config = AppConfig {
            telegram: TelegramConfig {
                bot_token: "test".to_string(),
                chat_id: "test".to_string(),
            },
            scripts_dir: None,
            jobs,
        };

        let job_states = HashMap::new();
        let next_wake = calculate_next_wake_time(&config, &job_states);
        let now = Utc::now();

        // Next wake should be soon (within 5 minutes because of the frequent job)
        assert!(next_wake > now);
        assert!(next_wake < now + chrono::Duration::minutes(6));
    }

    #[test]
    fn test_calculate_next_wake_time_disabled_jobs() {
        use crate::config::JobConfig;
        use std::collections::HashMap;

        let mut jobs = HashMap::new();

        // Only disabled jobs
        jobs.insert(
            "disabled".to_string(),
            JobConfig {
                schedule: "*/5 * * * *".to_string(),
                script: "test.sh".to_string(),
                timeout: 60,
                enabled: false, // Disabled
                args: vec![],
                env: HashMap::new(),
            },
        );

        let config = AppConfig {
            telegram: TelegramConfig {
                bot_token: "test".to_string(),
                chat_id: "test".to_string(),
            },
            scripts_dir: None,
            jobs,
        };

        let job_states = HashMap::new();
        let next_wake = calculate_next_wake_time(&config, &job_states);
        let now = Utc::now();

        // Should default to 1 minute from now when no enabled jobs
        let expected = now + chrono::Duration::minutes(1);
        let diff = (next_wake - expected).num_seconds().abs();
        assert!(diff < 2); // Within 2 seconds tolerance
    }
}
