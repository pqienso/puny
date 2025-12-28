use anyhow::{Context, Result};
use std::io::{BufReader, Read};
use std::process::{Child, Command, Stdio};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::config::DisplayConfig;

pub struct FrameCapture {
    display_name: String,
    width: u32,
    height: u32,
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
        Self {
            display_name: config.name.clone(),
            width: config.width,
            height: config.height,
            process: None,
        }
    }

    pub fn start(&mut self, frame_tx: mpsc::Sender<RawFrame>) -> Result<()> {
        info!("Starting frame capture for {}", self.display_name);

        // Calculate expected frame size (BGRA = 4 bytes per pixel)
        let frame_size = (self.width * self.height * 4) as usize;

        // Start wf-recorder in raw output mode
        let mut child = Command::new("wf-recorder")
            .args([
                "-o",
                &self.display_name,
                "-c",
                "rawvideo",
                "-p",
                "format=bgra",
                "-m",
                "null",
                "-f",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn wf-recorder")?;

        let stdout = child.stdout.take().context("Failed to get stdout")?;
        self.process = Some(child);

        // Spawn thread to read frames
        let display_name = self.display_name.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut frame_buffer = vec![0u8; frame_size];
            let mut frame_count = 0u64;

            info!("Frame capture thread started for {}", display_name);

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
                            width: frame_size as u32 / 4 / 1080, // TODO: Fix this
                            height: 1080,
                            timestamp_us: timestamp,
                        };

                        if frame_tx.blocking_send(frame).is_err() {
                            error!("Frame channel closed, stopping capture");
                            break;
                        }

                        frame_count += 1;
                        if frame_count >= 60 {
                            info!("Captured {} frames", frame_count);
                            frame_count = 0;
                        }
                    }
                    Err(e) => {
                        error!("Failed to read frame: {}", e);
                        break;
                    }
                }
            }

            info!("Frame capture thread stopped");
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
