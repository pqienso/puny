use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::DisplayConfig;

pub struct FrameCapture {
    display_name: String,
    width: u32,
    height: u32,
    refresh_rate: u32,
    process: Option<Child>,
}

#[derive(Debug)]
pub struct RawFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timestamp_us: u64,
}

impl FrameCapture {
    pub fn new(config: &DisplayConfig) -> Self {
        let (width, height, refresh_rate) = config
            .get_dimensions()
            .expect("Could not detect monitor dimensions");
        Self {
            display_name: config.name.clone(),
            width,
            height,
            refresh_rate,
            process: None,
        }
    }

    pub fn start(&mut self, frame_tx: mpsc::Sender<RawFrame>) -> Result<()> {
        info!("Starting frame capture for {}", self.display_name);

        // Start wf-recorder in raw output mode
        // Note: We use muxer "rawvideo" instead of "null" to properly output raw frames
        info!("Spawning wf-recorder for output: {}", self.display_name);
        let mut child = Command::new("wf-recorder")
            .args([
                "-o",
                &self.display_name,
                "-c",
                "rawvideo",
                "-p",
                "format=bgra",
                "-m",
                "rawvideo", // Use rawvideo muxer instead of null
                "-f",
                "pipe:1",
                "-r",
                self.refresh_rate.to_string().as_str(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()) // Capture stderr to see errors
            .spawn()
            .context("Failed to spawn wf-recorder")?;

        info!("wf-recorder spawned with PID: {}", child.id());

        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let stderr = child.stderr.take().context("Failed to get stderr")?;
        self.process = Some(child);

        // Spawn thread to monitor stderr
        let display_name_stderr = self.display_name.clone();
        std::thread::spawn(move || {
            let mut stderr_reader = BufReader::new(stderr);
            let mut line = String::new();

            while let Ok(n) = stderr_reader.read_line(&mut line) {
                if n == 0 {
                    break;
                }
                warn!("wf-recorder [{}]: {}", display_name_stderr, line.trim());
                line.clear();
            }
        });

        // Spawn thread to read frames
        let display_name = self.display_name.clone();
        let width = self.width;
        let height = self.height;

        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut frame_count = 0u64;

            let frame_size = (width * height * 4) as usize; // BGRA = 4 bytes per pixel
            let mut frame_buffer = vec![0u8; frame_size];

            info!(
                "Frame capture thread started for {} ({}x{}, {} bytes per frame)",
                display_name, width, height, frame_size
            );

            loop {
                // Read exact frame size
                match reader.read_exact(&mut frame_buffer) {
                    Ok(_) => {
                        let timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_micros() as u64;

                        let frame = RawFrame {
                            data: frame_buffer.clone(),
                            width,
                            height,
                            timestamp_us: timestamp,
                        };

                        if frame_tx.blocking_send(frame).is_err() {
                            error!("Frame channel closed, stopping capture");
                            break;
                        }

                        frame_count += 1;
                        if frame_count.is_multiple_of(60) {
                            info!("Captured {} frames", frame_count);
                        }
                    }
                    Err(e) => {
                        error!("Failed to read frame: {}", e);
                        break;
                    }
                }
            }

            info!("Frame capture thread stopped after {} frames", frame_count);
        });

        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        if let Some(mut process) = self.process.take() {
            info!("Stopping frame capture");
            process.kill()?;
            process.wait()?;
        }
        Ok(())
    }
}

impl Drop for FrameCapture {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
