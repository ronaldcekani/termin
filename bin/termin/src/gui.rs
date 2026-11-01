//! GUI application launch (`bin/termin/src/gui.rs`).

use std::path::PathBuf;
use termin_app::run;
use termin_config::{load, Config};

pub fn run_gui() -> ! {
    let config_path = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
            PathBuf::from(home).join(".config")
        })
        .join("termin")
        .join("config.toml");

    let (config, diagnostics) = if config_path.exists() {
        load(&config_path)
    } else {
        (Config::default(), Vec::new())
    };

    for diag in diagnostics {
        log::warn!("Config diagnostic: {}", diag.message);
    }

    run(config)
}
