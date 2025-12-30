use crate::capture::RawFrame;
use crate::config::EncoderConfig;
use crate::hw_detect::{EncoderDetector, HardwareEncoder};

use anyhow::{Context, Result};
use std::io::{BufRead, BufReader};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

pub struct VideoEncoder {
    config: Arc<EncoderConfig>,
    width: u32,
    height: u32,
    frame_rate: u32,
    selected_encoder: HardwareEncoder,
}

#[derive(Debug, Clone)]
pub struct EncodedPacket {
    pub data: Vec<u8>,
    pub pts: i64,
    pub dts: i64,
    pub is_keyframe: bool,
    pub timestamp_us: u64,
}

impl VideoEncoder {
    pub fn new(config: EncoderConfig, width: u32, height: u32, frame_rate: u32) -> Result<Self> {
        // Detect available hardware encoders
        let detector = EncoderDetector::new()?;

        info!("Available encoders:");
        for encoder in detector.list_available() {
            let hw_marker = if encoder.is_hardware() { "⚡" } else { "💻" };
            info!(
                "  {} {} (priority: {})",
                hw_marker,
                encoder.codec_name(),
                encoder.priority()
            );
        }

        // Select best encoder
        let selected_encoder = detector
            .get_best_encoder()
            .context("No suitable encoder found")?
            .clone();

        info!(
            "Selected encoder: {} ({})",
            selected_encoder.codec_name(),
            if selected_encoder.is_hardware() {
                "Hardware"
            } else {
                "Software"
            }
        );

        Ok(Self {
            config: Arc::new(config),
            width,
            height,
            frame_rate,
            selected_encoder,
        })
    }

    pub fn start(
        &self,
        frame_rx: mpsc::Receiver<RawFrame>,
        packet_tx: mpsc::Sender<EncodedPacket>,
    ) -> Result<()> {
        info!("Starting video encoder");
        info!("  Encoder: {}", self.selected_encoder.codec_name());
        info!(
            "  Resolution: {}x{}@{}",
            self.width, self.height, self.frame_rate
        );
        info!("  Bitrate: {}kbps", self.config.bitrate_kbps);
        info!("  Preset: {:?}", self.config.preset);

        // Spawn encoding thread
        let config = self.config.clone();
        let width = self.width;
        let height = self.height;
        let frame_rate = self.frame_rate;
        let encoder = self.selected_encoder.clone();

        std::thread::spawn(move || {
            if let Err(e) = Self::encode_thread(
                config, width, height, frame_rate, encoder, frame_rx, packet_tx,
            ) {
                error!("Encoder thread failed: {}", e);
            }
        });

        Ok(())
    }

    fn encode_thread(
        config: Arc<EncoderConfig>,
        width: u32,
        height: u32,
        frame_rate: u32,
        encoder: HardwareEncoder,
        mut frame_rx: mpsc::Receiver<RawFrame>,
        packet_tx: mpsc::Sender<EncodedPacket>,
    ) -> Result<()> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        info!("Spawning FFmpeg encoder process");

        // Create new detector to build args
        let detector = EncoderDetector::new()?;

        // Build FFmpeg arguments using the detector
        let ffmpeg_args = detector.build_ffmpeg_args(
            &encoder,
            width,
            height,
            frame_rate,
            config.bitrate_kbps,
            &config.preset,
            config.keyframe_interval,
        );

        info!("FFmpeg command: ffmpeg {}", ffmpeg_args.join(" "));

        // Spawn FFmpeg process
        let mut child = Command::new("ffmpeg")
            .args(&ffmpeg_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn FFmpeg")?;

        let mut stdin = child.stdin.take().context("Failed to get stdin")?;
        let stderr = child.stderr.take().context("Failed to get stderr")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;

        // Thread to monitor stderr
        std::thread::spawn(move || {
            let mut stderr_reader = BufReader::new(stderr);
            let mut line = String::new();
            while let Ok(n) = stderr_reader.read_line(&mut line) {
                if n == 0 {
                    break;
                }
                let trimmed = line.trim();
                // Only show important FFmpeg messages
                if trimmed.contains("error") || trimmed.contains("Error") {
                    error!("[ffmpeg] {}", trimmed);
                } else if trimmed.contains("warning") || trimmed.contains("Warning") {
                    warn!("[ffmpeg] {}", trimmed);
                } else if !trimmed.is_empty()
                    && !trimmed.starts_with("frame=")
                    && !trimmed.contains("fps=")
                {
                    info!("[ffmpeg] {}", trimmed);
                }
                line.clear();
            }
        });

        // Spawn thread to write frames to FFmpeg
        let write_handle = std::thread::spawn(move || {
            let mut frame_count = 0u64;

            while let Some(frame) = frame_rx.blocking_recv() {
                if let Err(e) = stdin.write_all(&frame.data) {
                    error!("Failed to write frame to encoder: {}", e);
                    break;
                }

                frame_count += 1;
                if frame_count == 1 {
                    info!("✓ First frame sent to encoder");
                } else if frame_count.is_multiple_of(300) {
                    info!("Sent {} frames to encoder", frame_count);
                }
            }

            info!("Frame writing thread stopped after {} frames", frame_count);
        });

        // Read encoded packets from FFmpeg
        use std::io::Read;
        let mut reader = std::io::BufReader::new(stdout);
        let mut packet_count = 0u64;
        let mut buffer = vec![0u8; 1024 * 1024]; // 1MB buffer for packets

        info!("Starting to read encoded packets");

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    info!("FFmpeg encoder closed");
                    break;
                }
                Ok(n) => {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_micros() as u64;

                    let packet = EncodedPacket {
                        data: buffer[..n].to_vec(),
                        pts: packet_count as i64,
                        dts: packet_count as i64,
                        is_keyframe: packet_count.is_multiple_of(config.keyframe_interval as u64),
                        timestamp_us: timestamp,
                    };

                    if packet_tx.blocking_send(packet).is_err() {
                        error!("Packet channel closed, stopping encoder");
                        break;
                    }

                    packet_count += 1;
                    if packet_count == 1 {
                        info!("✓ First encoded packet ready!");
                    } else if packet_count.is_multiple_of(300) {
                        info!("Encoded {} packets", packet_count);
                    }
                }
                Err(e) => {
                    error!("Failed to read encoded packet: {}", e);
                    break;
                }
            }
        }

        info!("Encoder thread stopped after {} packets", packet_count);
        let _ = write_handle.join();
        let _ = child.wait();

        Ok(())
    }
}
