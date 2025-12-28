mod config;
mod display;
mod capture;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{info, error};
use tracing_subscriber;

use config::PunyConfig;
use display::VirtualDisplay;
use capture::{FrameCapture, RawFrame};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🚀 Puny Daemon Starting");

    // Load configuration
    let config = PunyConfig::default();
    
    info!("Configuration:");
    info!("  Display: {}x{}@{}Hz", 
        config.display.width, 
        config.display.height, 
        config.display.refresh_rate
    );
    info!("  Encoder: {:?} @ {}kbps", 
        config.encoder.codec, 
        config.encoder.bitrate_kbps
    );

    // Create virtual display
    let mut display = VirtualDisplay::new(config.display.clone());
    display.create()?;

    // Verify display was created
    if !display.verify()? {
        anyhow::bail!("Failed to verify display creation");
    }

    // Setup frame capture channel
    let (frame_tx, mut frame_rx) = mpsc::channel::<RawFrame>(4);

    // Start frame capture
    let mut capture = FrameCapture::new(&config.display);
    capture.start(frame_tx)?;

    info!("✓ Capture pipeline initialized");
    info!("📡 Press Ctrl+C to stop");

    // Setup shutdown handler
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    // Main loop - process frames
    let mut frame_count = 0u64;
    let mut last_report = std::time::Instant::now();

    loop {
        tokio::select! {
            Some(frame) = frame_rx.recv() => {
                frame_count += 1;

                // Report FPS every second
                let now = std::time::Instant::now();
                if now.duration_since(last_report).as_secs() >= 1 {
                    let fps = frame_count as f64 / now.duration_since(last_report).as_secs_f64();
                    info!("Receiving frames at {:.1} FPS ({}x{} bytes: {})", 
                        fps, 
                        frame.width, 
                        frame.height,
                        frame.data.len()
                    );
                    frame_count = 0;
                    last_report = now;
                }

                // TODO: Send to encoder
            }
            _ = &mut shutdown => {
                info!("Shutdown signal received");
                break;
            }
        }
    }

    // Cleanup
    info!("Cleaning up...");
    capture.stop()?;
    display.remove()?;
    info!("✓ Shutdown complete");

    Ok(())
}
