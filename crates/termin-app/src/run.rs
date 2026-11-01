//! Application runner (`termin-app::run`).
//!
//! Entry point initializing configuration, platform run loop, and event handling.
//! See `docs/architecture.md §4` and `docs/specs/platform-macos.md`.

use std::sync::Arc;
use termin_config::Config;
use termin_platform::{DefaultPlatform, Platform};

use crate::app::App;

/// Run the Termin GUI application with configuration.
pub fn run(config: Config) -> ! {
    let app = App::new(Arc::new(config));
    DefaultPlatform::run(app)
}
