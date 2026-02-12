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

        thread::sleep(Duration::from_secs(5));
    }
}

#[cfg(test)]
mod tests {
    use chrono::Timelike;

    use super::*;

    #[test]
    fn test_should_fire_first_tick() {
        // Every hour at minute 0
        let schedule = Schedule::from_str("0 * * * *").unwrap();
        let now = Utc::now();

        // On first tick with no last_fired, should look back 1 minute
        // This will only fire if we're currently at minute 0
        let fires = should_fire(&schedule, None, now);

        // We can't guarantee what minute it is, so just test the logic works
        assert!(fires == (now.minute() == 0 || now.minute() == 1));
    }

    #[test]
    fn test_should_fire_after_time() {
        let schedule = Schedule::from_str("0 * * * *").unwrap();

        // If last fired was 2 hours ago, should fire now if we're at minute 0
        let two_hours_ago = Utc::now() - chrono::Duration::hours(2);
        let now = Utc::now();

        // Should have multiple scheduled times between then and now
        let fires = should_fire(&schedule, Some(two_hours_ago), now);
        assert!(fires); // At least one hour boundary crossed
    }
}
