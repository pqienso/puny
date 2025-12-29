mod capture;
mod config;
mod display;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{info, warn};

use capture::{FrameCapture, RawFrame};
use config::PunyConfig;
use display::VirtualDisplay;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🚀 Puny Daemon Starting");

    // Load configuration
    let config = PunyConfig::default();

    // Log configuration details
    info!("Configuration:");
    info!("  Display: {}", config.display.name);
    info!("  Resolution: {}", config.display.resolution);
    info!("  Position: {}", config.display.position);
    info!("  Scale: {}", config.display.scale);
    info!(
        "  Encoder: {:?} @ {}kbps",
        config.encoder.codec, config.encoder.bitrate_kbps
    );
    info!("  Preset: {:?}", config.encoder.preset);

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
    info!("📡 Waiting for frames... Press Ctrl+C to stop");

    // Setup shutdown handler
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    // Main loop - process frames
    let mut frame_count = 0u64;
    let mut last_report = std::time::Instant::now();
    let mut total_bytes = 0u64;
    let start_time = std::time::Instant::now();
    let mut first_frame_received = false;

    // Timeout warning
    let mut timeout_fired = false;

    loop {
        let timeout = tokio::time::sleep(tokio::time::Duration::from_secs(5));

        tokio::select! {
            Some(frame) = frame_rx.recv() => {
                if !first_frame_received {
                    first_frame_received = true;
                    let startup_time = start_time.elapsed();
                    info!("✓ First frame received after {:.2}s!", startup_time.as_secs_f64());
                }

                frame_count += 1;
                total_bytes += frame.data.len() as u64;

                // Report stats every second
                let now = std::time::Instant::now();
                let elapsed = now.duration_since(last_report);

                if elapsed.as_secs() >= 1 {
                    let fps = frame_count as f64 / elapsed.as_secs_f64();
                    let mbps = (total_bytes as f64 * 8.0) / (elapsed.as_secs_f64() * 1_000_000.0);

                    info!(
                        "📊 Stats: {:.1} FPS | {}x{} | {:.1} Mbps raw",
                        fps,
                        frame.width,
                        frame.height,
                        mbps
                    );

                    frame_count = 0;
                    total_bytes = 0;
                    last_report = now;
                }

                // TODO: Send to encoder
            }
            _ = timeout, if !first_frame_received && !timeout_fired => {
                timeout_fired = true;
                warn!("⚠️  No frames received after 5 seconds.");
                warn!("   • Check wf-recorder stderr output above");
                warn!("   • Try moving a window to the {} display", config.display.name);
                warn!("   • Run: hyprctl dispatch moveworkspacetomonitor 1 {}", config.display.name);
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
