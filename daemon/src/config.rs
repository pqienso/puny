use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PunyConfig {
    pub display: DisplayConfig,
    pub encoder: EncoderConfig,
    pub transport: TransportConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    pub name: String,
    #[serde(flatten)]
    pub resolution: Resolution,
    #[serde(flatten)]
    pub position: Position,
    pub scale: f32,
}

// Resolution includes both dimensions and refresh rate
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Resolution {
    Explicit {
        width: u32,
        height: u32,
        #[serde(default = "default_refresh_rate")]
        refresh_rate: u32,
    },
    Keyword(String),
}

fn default_refresh_rate() -> u32 {
    60
}

impl Resolution {
    // Convenience constructors
    pub fn dimensions(width: u32, height: u32, refresh_rate: u32) -> Self {
        Self::Explicit {
            width,
            height,
            refresh_rate,
        }
    }

    pub fn auto() -> Self {
        Self::Keyword("auto".to_string())
    }

    pub fn preferred() -> Self {
        Self::Keyword("preferred".to_string())
    }

    pub fn highres() -> Self {
        Self::Keyword("highres".to_string())
    }

    pub fn highrr() -> Self {
        Self::Keyword("highrr".to_string())
    }

    pub fn max_width() -> Self {
        Self::Keyword("max".to_string())
    }

    // Helper to get dimensions if explicit
    pub fn get_dimensions(&self) -> Option<(u32, u32, u32)> {
        match self {
            Self::Explicit {
                width,
                height,
                refresh_rate,
            } => Some((*width, *height, *refresh_rate)),
            Self::Keyword(_) => None,
        }
    }
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Explicit {
                width,
                height,
                refresh_rate,
            } => {
                write!(f, "{}x{}@{}", width, height, refresh_rate)
            }
            Self::Keyword(kw) => write!(f, "{}", kw),
        }
    }
}

// Position can be explicit coordinates or a string keyword
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Position {
    Coordinates { x: i32, y: i32 },
    Keyword(String),
}

impl Position {
    // Convenience constructors
    pub fn coords(x: i32, y: i32) -> Self {
        Self::Coordinates { x, y }
    }

    pub fn auto() -> Self {
        Self::Keyword("auto".to_string())
    }

    pub fn auto_right() -> Self {
        Self::Keyword("auto-right".to_string())
    }

    pub fn auto_left() -> Self {
        Self::Keyword("auto-left".to_string())
    }

    pub fn auto_up() -> Self {
        Self::Keyword("auto-up".to_string())
    }

    pub fn auto_down() -> Self {
        Self::Keyword("auto-down".to_string())
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Coordinates { x, y } => write!(f, "{}x{}", x, y),
            Self::Keyword(kw) => write!(f, "{}", kw),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderConfig {
    pub codec: Codec,
    pub bitrate_kbps: u32,
    pub preset: EncoderPreset,
    pub keyframe_interval: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    H264,
    H265,
    AV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncoderPreset {
    UltraLowLatency,
    LowLatency,
    Balanced,
    HighQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    pub protocol: TransportProtocol,
    pub port: u16,
    pub buffer_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportProtocol {
    Udp,
    Quic,
    Tcp,
}

impl Default for PunyConfig {
    fn default() -> Self {
        Self {
            display: DisplayConfig {
                name: "PUNY-1".to_string(),
                resolution: Resolution::dimensions(1920, 1080, 60),
                position: Position::auto(),
                scale: 1.0,
            },
            encoder: EncoderConfig {
                codec: Codec::H264,
                bitrate_kbps: 8000,
                preset: EncoderPreset::UltraLowLatency,
                keyframe_interval: 60,
            },
            transport: TransportConfig {
                protocol: TransportProtocol::Udp,
                port: 12345,
                buffer_size: 1024 * 1024,
            },
        }
    }
}

impl DisplayConfig {
    pub fn to_hyprland_string(&self) -> String {
        format!(
            "{},{},{},{}",
            self.name, self.resolution, self.position, self.scale
        )
    }

    // Helper to get actual dimensions (useful for frame capture)
    pub fn get_dimensions(&self) -> Result<(u32, u32, u32)> {
        match self.resolution.get_dimensions() {
            Some((width, height, refresh_rate)) => Ok((width, height, refresh_rate)),
            None => self.query_display_dimensions(),
        }
    }

    fn query_display_dimensions(&self) -> Result<(u32, u32, u32)> {
        let output = Command::new("hyprctl")
            .args(["monitors", "-j"])
            .output()
            .context("Failed to query monitors")?;

        if !output.status.success() {
            anyhow::bail!("hyprctl command failed");
        }

        let json_str =
            String::from_utf8(output.stdout).context("Invalid UTF-8 in hyprctl output")?;

        let monitors: serde_json::Value =
            serde_json::from_str(&json_str).context("Failed to parse hyprctl JSON")?;

        if let Some(monitors_array) = monitors.as_array() {
            for monitor in monitors_array {
                if let Some(name) = monitor.get("name").and_then(|n| n.as_str()) {
                    if name == self.name {
                        let width = monitor
                            .get("width")
                            .and_then(|w| w.as_u64())
                            .context("Missing width field")?
                            as u32;
                        let height = monitor
                            .get("height")
                            .and_then(|h| h.as_u64())
                            .context("Missing height field")?
                            as u32;
                        let refresh_rate = monitor
                            .get("refreshRate")
                            .and_then(|rr| rr.as_u64())
                            .context("Missing refresh rate field")?
                            as u32;

                        return Ok((width, height, refresh_rate));
                    }
                }
            }
        }

        anyhow::bail!("Display {} not found in monitor list", self.name)
    }
}

// Example usage:
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_config_string() {
        let config = DisplayConfig {
            name: "PUNY-1".to_string(),
            resolution: Resolution::dimensions(1920, 1080, 60),
            position: Position::coords(1920, 0),
            scale: 1.0,
        };

        assert_eq!(
            config.to_hyprland_string(),
            "PUNY-1,1920x1080@60,1920x0,1.0"
        );

        let config2 = DisplayConfig {
            name: "PUNY-2".to_string(),
            resolution: Resolution::preferred(),
            position: Position::auto_right(),
            scale: 1.5,
        };

        assert_eq!(
            config2.to_hyprland_string(),
            "PUNY-2,preferred,auto-right,1.5"
        );

        let config3 = DisplayConfig {
            name: "PUNY-3".to_string(),
            resolution: Resolution::auto(),
            position: Position::auto(),
            scale: 1.0,
        };

        assert_eq!(config3.to_hyprland_string(), "PUNY-3,auto,auto,1.0");
    }
}
