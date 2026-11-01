//! Sidebar metadata collectors. Results are intended to be applied on the main thread.

pub mod git;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Reasons metadata needs refreshing. The runtime drives these without doing
/// filesystem or process work in the frame callback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RefreshTriggers {
    pub cwd_changed: bool,
    pub command_finished: bool,
    pub command_started: bool,
    pub output_burst_ended: bool,
}

/// Per-workspace polling state. A hidden or occluded sidebar never schedules
/// periodic port scans; one-shot cwd/command events can still refresh git data.
#[derive(Clone, Debug, Default)]
pub struct MetadataSchedule {
    next_ports_at: Option<Instant>,
    next_foreground_at: Option<Instant>,
    last_git_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DueMetadata {
    pub git: bool,
    pub foreground_process: bool,
    pub ports: bool,
}

impl MetadataSchedule {
    pub const PORT_INTERVAL: Duration = Duration::from_secs(5);
    pub const FOREGROUND_DEBOUNCE: Duration = Duration::from_millis(300);
    pub const COMMAND_START_PORT_DELAY: Duration = Duration::from_secs(2);
    pub const GIT_MIN_INTERVAL: Duration = Duration::from_secs(5);

    /// Decide which cold collectors may run now and advance their deadlines.
    pub fn poll(
        &mut self,
        now: Instant,
        sidebar_visible: bool,
        window_occluded: bool,
        triggers: RefreshTriggers,
    ) -> DueMetadata {
        let active = sidebar_visible && !window_occluded;
        let git_triggered = triggers.cwd_changed || triggers.command_finished;
        let git = git_triggered
            && self.last_git_at.map_or(true, |last| {
                triggers.cwd_changed
                    || now.saturating_duration_since(last) >= Self::GIT_MIN_INTERVAL
            });
        if git {
            self.last_git_at = Some(now);
        }

        if triggers.output_burst_ended {
            self.next_foreground_at = Some(now + Self::FOREGROUND_DEBOUNCE);
        }
        if triggers.command_started && active {
            self.next_ports_at = Some(now + Self::COMMAND_START_PORT_DELAY);
        }
        if !active {
            self.next_ports_at = None;
        }
        if active && self.next_ports_at.is_none() {
            self.next_ports_at = Some(now + Self::PORT_INTERVAL);
        }

        let foreground_process = active
            && self
                .next_foreground_at
                .is_some_and(|deadline| now >= deadline);
        if foreground_process {
            self.next_foreground_at = None;
        }
        let ports = active && self.next_ports_at.is_some_and(|deadline| now >= deadline);
        if ports {
            self.next_ports_at = Some(now + Self::PORT_INTERVAL);
        }
        DueMetadata {
            git,
            foreground_process,
            ports,
        }
    }
}

/// Build distinct cwd labels in priority order, preserving focused cwd first.
/// The caller limits the returned list to the three paths shown in the sidebar.
pub fn distinct_paths<'a>(
    focused: Option<&'a str>,
    paths: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let mut result = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for path in focused.into_iter().chain(paths) {
        let path = Path::new(path);
        if path.as_os_str().is_empty() {
            continue;
        }
        // Lexical normalization is sufficient for duplicate display entries and
        // avoids filesystem IO and symlink resolution on the main thread.
        let normalized = lexical_path(path);
        if !seen.contains(&normalized) {
            result.push(path.to_string_lossy().into_owned());
            seen.push(normalized);
        }
        if result.len() == 3 {
            break;
        }
    }
    result
}

fn lexical_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_list_deduplicates_and_prioritizes_focused_path() {
        assert_eq!(
            distinct_paths(
                Some("/tmp/project/./src"),
                ["/tmp/project/src", "/tmp/other"]
            ),
            ["/tmp/project/./src", "/tmp/other"]
        );
    }

    #[test]
    fn port_polling_stops_for_hidden_or_occluded_sidebars() {
        let start = Instant::now();
        let mut schedule = MetadataSchedule::default();
        assert_eq!(
            schedule.poll(start, false, false, RefreshTriggers::default()),
            DueMetadata::default()
        );
        assert_eq!(
            schedule.poll(
                start + Duration::from_secs(20),
                true,
                true,
                RefreshTriggers::default()
            ),
            DueMetadata::default()
        );
        assert!(
            schedule
                .poll(
                    start + Duration::from_secs(20),
                    true,
                    false,
                    RefreshTriggers::default()
                )
                .ports
                == false
        );
        assert!(
            schedule
                .poll(
                    start + Duration::from_secs(25),
                    true,
                    false,
                    RefreshTriggers::default()
                )
                .ports
        );
    }

    #[test]
    fn command_start_schedules_port_scan_two_seconds_later() {
        let now = Instant::now();
        let mut schedule = MetadataSchedule::default();
        let trigger = RefreshTriggers {
            command_started: true,
            ..RefreshTriggers::default()
        };
        assert!(!schedule.poll(now, true, false, trigger).ports);
        assert!(
            schedule
                .poll(
                    now + Duration::from_secs(2),
                    true,
                    false,
                    RefreshTriggers::default()
                )
                .ports
        );
    }
}
