use crate::capture::RawFrame;
use crate::config::EncoderConfig;

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
    pub fn new(config: EncoderConfig, width: u32, height: u32, frame_rate: u32) -> Self {
        Self {
            config: Arc::new(config),
            width,
            height,
            frame_rate,
        }
    }

    pub fn start(
        &self,
        mut frame_rx: mpsc::Receiver<RawFrame>,
        packet_tx: mpsc::Sender<EncodedPacket>,
    ) -> Result<()> {
        info!("Starting H.264 encoder");
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

        std::thread::spawn(move || {
            if let Err(e) =
                Self::encode_thread(config, width, height, frame_rate, frame_rx, packet_tx)
            {
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
        mut frame_rx: mpsc::Receiver<RawFrame>,
        packet_tx: mpsc::Sender<EncodedPacket>,
    ) -> Result<()> {
        // For now, we'll use x264enc via FFmpeg command line
        // In Phase 2.5, we can optimize to use ffmpeg-next bindings directly
        use std::io::Write;
        use std::process::{Command, Stdio};
        info!("Spawning FFmpeg encoder process");
        // Build FFmpeg command for H.264 encoding
        let preset = match config.preset {
            crate::config::EncoderPreset::UltraLowLatency => "ultrafast",
            crate::config::EncoderPreset::LowLatency => "veryfast",
            crate::config::EncoderPreset::Balanced => "medium",
            crate::config::EncoderPreset::HighQuality => "slow",
        };
        let bitrate = format!("{}k", config.bitrate_kbps);
        let framerate_str = frame_rate.to_string();
        let mut child = Command::new("ffmpeg")
            .args([
                "-f",
                "rawvideo",
                "-pix_fmt",
                "bgra",
                "-s",
                &format!("{}x{}", width, height),
                "-r",
                &framerate_str,
                "-i",
                "pipe:0",
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p3", // p1 is fastest/lowest latency for NVENC
                "-tune",
                "hq", // Ultra-low latency
                "-zerolatency",
                "1",
                "-b:v",
                &bitrate,
                "-g",
                &config.keyframe_interval.to_string(),
                "-bf",
                "0",
                "-f",
                "h264",
                "pipe:1",
            ])
            // .args([
            //     "-f",
            //     "rawvideo",
            //     "-pix_fmt",
            //     "bgra",
            //     "-s",
            //     &format!("{}x{}", width, height),
            //     "-r",
            //     &framerate_str,
            //     "-i",
            //     "pipe:0",
            //     "-c:v",
            //     "libx264",
            //     "-preset",
            //     preset,
            //     "-tune",
            //     "zerolatency",
            //     "-b:v",
            //     &bitrate,
            //     "-g",
            //     &config.keyframe_interval.to_string(),
            //     "-bf",
            //     "0", // No B-frames for low latency
            //     "-pix_fmt",
            //     "yuv420p",
            //     "-f",
            //     "h264",
            //     "pipe:1",
            // ])
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
                warn!("[ffmpeg] : {}", line.trim());
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
                if frame_count.is_multiple_of(60) {
                    info!("Encoded {} frames", frame_count);
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
