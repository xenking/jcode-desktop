use std::time::{Duration, Instant};

pub(crate) const FOREGROUND_TICK: Duration = Duration::from_secs(1);
pub(crate) const BACKGROUND_TICK: Duration = Duration::from_secs(10);
const REFOCUS_MIN_AGE: Duration = Duration::from_secs(5);
const PICKER_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
pub(super) const CHECKPOINT_HEARTBEAT: Duration = Duration::from_secs(10);

pub(super) fn should_checkpoint(age: Duration, changed: bool) -> bool {
    changed || age >= CHECKPOINT_HEARTBEAT
}

pub(crate) struct CatalogRefresh {
    last: Instant,
    interval: Duration,
    next_interval: Duration,
}

impl CatalogRefresh {
    pub(crate) fn new(now: Instant, interval: Duration) -> Self {
        Self {
            last: now,
            interval,
            next_interval: interval.min(Duration::from_secs(2)),
        }
    }

    pub(crate) fn due(
        &mut self,
        now: Instant,
        focused: bool,
        activated: bool,
        picker_open: bool,
    ) -> bool {
        let age = now.duration_since(self.last);
        let interval = if picker_open {
            self.next_interval.min(PICKER_REFRESH_INTERVAL)
        } else {
            self.next_interval
        };
        if !focused || (age < interval && !(activated && age >= REFOCUS_MIN_AGE)) {
            return false;
        }
        self.last = now;
        self.next_interval = self.interval;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_catalog_stops_polling_and_focus_reconciles_once() {
        let start = Instant::now();
        let seconds = |n| start + Duration::from_secs(n);
        let mut schedule = CatalogRefresh::new(start, Duration::from_secs(5));
        let hidden_requests = (1..=60)
            .filter(|&second| schedule.due(seconds(second), false, false, true))
            .count();
        eprintln!("catalog requests per hidden minute: {hidden_requests}");
        assert_eq!(hidden_requests, 0, "catalog requests per hidden minute");
        assert!(schedule.due(seconds(61), true, true, false));
        assert!(!schedule.due(seconds(62), true, true, false), "focus bouncing");
        assert!(!schedule.due(seconds(65), true, false, false));
        assert!(schedule.due(seconds(66), true, false, false));
    }

    #[test]
    fn refocus_reconciles_early_without_resetting_the_timer_on_blur() {
        let start = Instant::now();
        let seconds = |n| start + Duration::from_secs(n);
        let mut schedule = CatalogRefresh::new(start, Duration::from_secs(60));
        assert!(schedule.due(seconds(2), true, false, false));
        assert!(!schedule.due(seconds(7), false, false, false));
        assert!(schedule.due(seconds(8), true, true, false));
        assert!(!schedule.due(seconds(9), true, true, false));
        assert!(!schedule.due(seconds(67), true, false, false));
        assert!(schedule.due(seconds(68), true, false, false));
    }

    #[test]
    fn foreground_picker_refreshes_live_rows_without_changing_the_idle_schedule() {
        let start = Instant::now();
        let seconds = |n| start + Duration::from_secs(n);
        let mut schedule = CatalogRefresh::new(start, Duration::from_secs(60));
        assert!(schedule.due(seconds(2), true, false, false));
        assert!(!schedule.due(seconds(6), true, false, true));
        assert!(schedule.due(seconds(7), true, false, true));
        assert!(!schedule.due(seconds(12), true, false, false));
        assert!(schedule.due(seconds(67), true, false, false));
    }

    #[test]
    fn unchanged_checkpoints_keep_a_sparse_heartbeat_and_edits_save_immediately() {
        let mut since_last_save = Duration::ZERO;
        let mut writes = 0;
        for _ in 1..=60 {
            since_last_save += Duration::from_secs(1);
            if should_checkpoint(since_last_save, false) {
                assert_eq!(since_last_save, CHECKPOINT_HEARTBEAT);
                writes += 1;
                since_last_save = Duration::ZERO;
            }
        }
        assert_eq!(writes, 6, "checkpoint writes per idle minute");
        assert!(should_checkpoint(Duration::from_secs(1), true));
    }
}
