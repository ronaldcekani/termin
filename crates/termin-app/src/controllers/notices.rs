//! Timed app banners and toasts (Phase 3C).
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Error,
    Info,
    Success,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    pub kind: NoticeKind,
    pub message: String,
    pub action: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub message: String,
}
#[derive(Clone, Debug)]
struct Timed<T> {
    value: T,
    at: Instant,
    duration: Duration,
}
#[derive(Debug, Default)]
pub struct Notices {
    pub banner: Option<Banner>,
    toast: Option<Timed<Toast>>,
}
impl Notices {
    pub fn show_error(&mut self, message: impl Into<String>) {
        self.banner = Some(Banner {
            kind: NoticeKind::Error,
            message: message.into(),
            action: None,
        });
    }
    pub fn show_info(&mut self, message: impl Into<String>, action: Option<String>) {
        self.banner = Some(Banner {
            kind: NoticeKind::Info,
            message: message.into(),
            action,
        });
    }
    pub fn dismiss_banner(&mut self) {
        self.banner = None;
    }
    pub fn toast(&mut self, message: impl Into<String>, now: Instant) {
        self.toast = Some(Timed {
            value: Toast {
                message: message.into(),
            },
            at: now,
            duration: Duration::from_millis(1600),
        });
    }
    pub fn active_toast(&mut self, now: Instant) -> Option<&Toast> {
        if self
            .toast
            .as_ref()
            .is_some_and(|t| now.saturating_duration_since(t.at) >= t.duration)
        {
            self.toast = None;
        }
        self.toast.as_ref().map(|t| &t.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn info_and_toast_expire_but_errors_stay() {
        let now = Instant::now();
        let mut n = Notices::default();
        n.show_info("updated", None);
        n.toast("copied", now);
        assert!(n.active_toast(now + Duration::from_secs(2)).is_none());
        n.show_error("bad config");
        assert_eq!(n.banner.as_ref().unwrap().kind, NoticeKind::Error);
    }
}
