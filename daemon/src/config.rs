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
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
    pub position: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
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
    UDP,
    QUIC,
    TCP,
}

impl Default for PunyConfig {
    fn default() -> Self {
        Self {
            display: DisplayConfig {
                name: "PUNY-1".to_string(),
                width: 1920,
                height: 1080,
                refresh_rate: 60,
                position: Position { x: -500, y: -5000 },
            },
            encoder: EncoderConfig {
                codec: Codec::H264,
                bitrate_kbps: 8000,
                preset: EncoderPreset::UltraLowLatency,
                keyframe_interval: 60, // Every 1 second at 60fps
            },
            transport: TransportConfig {
                protocol: TransportProtocol::UDP,
                port: 12345,
                buffer_size: 1024 * 1024, // 1MB
            },
        }
    }
}
