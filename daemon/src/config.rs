use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PunyConfig {
    pub display: DisplayConfig,
    pub encoder: EncoderConfig,
    pub transport: TransportConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    pub name: String,
    pub resolution: Resolution,
    pub refresh_rate: u32,
    pub position: Position,
    pub scale: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AutoResolution {
    Preferred,
    HighRes,
    HighRr,
    MaxWidth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Resolution {
    Numeric { width: u32, height: u32 },
    Special(AutoResolution),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AutoPosition {
    Auto,
    AutoRight,
    AutoLeft,
    AutoUp,
    AutoDown,
    AutoCenterRight,
    AutoCenterLeft,
    AutoCenterUp,
    AutoCenterDown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Position {
    Coordinates { x: i32, y: i32 },
    Special(AutoPosition),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderConfig {
    pub codec: Codec,
    pub bitrate_kbps: u32,
    pub preset: EncoderPreset,
    pub keyframe_interval: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Codec {
    H264,
    H265,
    AV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
                resolution: Resolution::Special(AutoResolution::Preferred),
                refresh_rate: 60,
                position: Position::Special(AutoPosition::Auto),
                scale: 1.0,
            },
            encoder: EncoderConfig {
                codec: Codec::H264,
                bitrate_kbps: 8000,
                preset: EncoderPreset::UltraLowLatency,
                keyframe_interval: 60, // Every 1 second at 60fps
            },
            transport: TransportConfig {
                protocol: TransportProtocol::Udp,
                port: 12345,
                buffer_size: 1024 * 1024, // 1MB
            },
        }
    }
}

impl DisplayConfig {
    pub fn to_hyprland_string(&self) -> String {
        let res_str = match &self.resolution {
            Resolution::Numeric { width, height } => format!("{}x{}", width, height),
            Resolution::Special(s) => serde_json::to_string(s)
                .unwrap_or_default()
                .replace('"', ""),
        };

        let pos_str = match &self.position {
            Position::Coordinates { x, y } => format!("{}x{}", x, y),
            Position::Special(s) => serde_json::to_string(s)
                .unwrap_or_default()
                .replace('"', ""),
        };

        format!(
            "{},{}@{},{},{}",
            self.name, res_str, self.refresh_rate, pos_str, self.scale
        )
    }
}
