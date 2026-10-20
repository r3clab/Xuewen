use std::sync::Arc;

use chrono::{DateTime, Timelike, Utc};
use tokio_util::sync::CancellationToken;

use super::{store, DailyService};

/// Parse `[daily].run_at` as 24h "HH:MM".
pub(crate) fn parse_run_at(s: &str) -> anyhow::Result<(u32, u32)> {
    let (h, m) = s
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("[daily].run_at must be \"HH:MM\", got {s:?}"))?;
    let (h, m): (u32, u32) = (h.parse()?, m.parse()?);
    if h > 23 || m > 59 {
        anyhow::bail!("[daily].run_at out of range: {s:?}");
    }
    Ok((h, m))
}

/// A run is due when we are past today's `run_at` and today's run is
/// missing (boot catch-up) or failed (hourly retry). "ok"/"empty" are done.
fn run_due(now: DateTime<Utc>, run_at: (u32, u32), today_status: Option<&str>) -> bool {
    let past = (now.hour(), now.minute()) >= run_at;
    past && !matches!(today_status, Some("ok") | Some("empty"))
}

/// Seconds until the next occurrence of `run_at` (UTC), capped at 3600 so
/// the loop re-checks hourly (which is what retries failed runs).
fn sleep_secs(now: DateTime<Utc>, run_at: (u32, u32)) -> u64 {
    let today_target = now
        .date_naive()
        .and_hms_opt(run_at.0, run_at.1, 0)
        .expect("validated by parse_run_at");
    let target = if today_target > now.naive_utc() {
        today_target
    } else {
        today_target + chrono::Days::new(1)
    };
    let secs = (target - now.naive_utc()).num_seconds().max(1) as u64;
    secs.min(3600)
}

/// Loop spawned by `serve`, until `cancel` fires: boot catch-up, the daily
/// scheduled run, and hourly retry after a failure — all same-day only (the
/// arXiv feed is a live window; there is no backfill).
pub(crate) async fn run(svc: Arc<DailyService>, cancel: CancellationToken) {
    let run_at = parse_run_at(&svc.cfg.run_at).expect("validated in from_config");
    loop {
        let now = Utc::now();
        let today = now.format("%Y-%m-%d").to_string();
        let status = match store::get_run(&svc.pool, &today).await {
            Ok(r) => r.map(|r| r.status),
            Err(e) => {
                tracing::error!("daily scheduler: reading run state: {e:#}");
                None
            }
        };
        if run_due(now, run_at, status.as_deref()) {
            if let Some(run) = svc.run_guarded(&today).await {
                tracing::info!(
                    "daily run {}: {} ({} candidates)",
                    run.batch_date,
                    run.status,
                    run.papers_found
                );
            }
        }
        let secs = sleep_secs(Utc::now(), run_at);
        tokio::select! {
            biased;
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(std::time::Duration::from_secs(secs)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 10, h, m, 0).unwrap()
    }

    #[test]
    fn parses_and_validates_run_at() {
        assert_eq!(parse_run_at("09:00").unwrap(), (9, 0));
        assert_eq!(parse_run_at("23:59").unwrap(), (23, 59));
        assert!(parse_run_at("24:00").is_err());
        assert!(parse_run_at("09:60").is_err());
        assert!(parse_run_at("0900").is_err());
        assert!(parse_run_at("morning").is_err());
    }

    #[test]
    fn run_due_only_after_run_at_and_not_after_success() {
        // Before run_at: never due.
        assert!(!run_due(at(8, 59), (9, 0), None));
        // After run_at with no run yet (boot catch-up): due.
        assert!(run_due(at(15, 0), (9, 0), None));
        // Failed earlier today: due again (hourly retry).
        assert!(run_due(at(10, 0), (9, 0), Some("failed")));
        // Succeeded (ok or empty): not due.
        assert!(!run_due(at(10, 0), (9, 0), Some("ok")));
        assert!(!run_due(at(10, 0), (9, 0), Some("empty")));
    }

    #[test]
    fn sleep_secs_targets_run_at_and_caps_at_one_hour() {
        // 08:30, run at 09:00 -> 30 minutes.
        assert_eq!(sleep_secs(at(8, 30), (9, 0)), 30 * 60);
        // 10:00, run at 09:00 -> next occurrence is tomorrow, capped hourly.
        assert_eq!(sleep_secs(at(10, 0), (9, 0)), 3600);
        // Exactly at run_at -> next occurrence tomorrow, capped.
        assert_eq!(sleep_secs(at(9, 0), (9, 0)), 3600);
    }
}
