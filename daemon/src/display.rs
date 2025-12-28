use anyhow::{Context, Result};
use std::process::Command;
use std::thread;
use std::time::Duration;
use tracing::{info, warn};

use crate::config::DisplayConfig;

pub struct VirtualDisplay {
    config: DisplayConfig,
    created: bool,
}

impl VirtualDisplay {
    pub fn new(config: DisplayConfig) -> Self {
        Self {
            config,
            created: false,
        }
    }

    pub fn create(&mut self) -> Result<()> {
        info!("Creating virtual display: {}", self.config.name);

        // Remove existing display if present
        let _ = self.remove_internal();
        thread::sleep(Duration::from_millis(300));

        // Create headless output
        let output = Command::new("hyprctl")
            .args(["output", "create", "headless", &self.config.name])
            .output()
            .context("Failed to execute hyprctl")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("already") {
                anyhow::bail!("Failed to create headless output: {}", stderr);
            }
        }

        thread::sleep(Duration::from_millis(500));

        // Configure monitor
        let monitor_config_str = self.config.to_hyprland_string();

        let output = Command::new("hyprctl")
            .args(["keyword", "monitor", &monitor_config_str])
            .output()
            .context("Failed to configure monitor")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to configure monitor: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        self.created = true;

        info!("✓ Virtual display created with config: {:?})", self.config);

        Ok(())
    }

    pub fn verify(&self) -> Result<bool> {
        let output = Command::new("hyprctl")
            .args(["monitors"])
            .output()
            .context("Failed to query monitors")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.contains(&self.config.name))
    }

    fn remove_internal(&self) -> Result<()> {
        Command::new("hyprctl")
            .args(["output", "remove", &self.config.name])
            .output()
            .context("Failed to remove display")?;
        Ok(())
    }

    pub fn remove(&mut self) -> Result<()> {
        if !self.created {
            return Ok(());
        }

        info!("Removing virtual display: {}", self.config.name);
        self.remove_internal()?;
        self.created = false;
        Ok(())
    }
}

impl Drop for VirtualDisplay {
    fn drop(&mut self) {
        if self.created {
            if let Err(e) = self.remove() {
                warn!("Failed to remove display on drop: {}", e);
            }
        }
    }
}
