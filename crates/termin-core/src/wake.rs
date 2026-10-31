//! Wake handle for signalling across threads.
//!
//! See `docs/architecture.md §3.5`.

use std::fmt;
use std::sync::Arc;

/// A thread-safe, cloneable handle used to wake the main thread run loop.
///
/// See `docs/architecture.md §3.5`.
#[derive(Clone)]
pub struct WakeHandle(Arc<dyn Fn() + Send + Sync>);

impl WakeHandle {
    /// Create a new `WakeHandle` from a wake closure.
    pub fn new(f: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }

    /// Signal the wake event.
    pub fn wake(&self) {
        (self.0)();
    }
}

impl fmt::Debug for WakeHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WakeHandle").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_wake_handle_invocation() {
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&flag);
        let handle = WakeHandle::new(move || {
            flag_clone.store(true, Ordering::SeqCst);
        });

        assert!(!flag.load(Ordering::SeqCst));
        handle.wake();
        assert!(flag.load(Ordering::SeqCst));
    }
}
