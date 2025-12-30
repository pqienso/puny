mod capture;
mod config;
mod display;
mod encoder;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{info, warn};

use capture::{FrameCapture, RawFrame};
use config::PunyConfig;
use display::VirtualDisplay;
use encoder::{EncodedPacket, VideoEncoder};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Puny Daemon Starting");

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
    if !display.verify()? {
        anyhow::bail!("Failed to verify display creation");
    }

    // Setup capture pipeline
    let (frame_tx, frame_rx) = mpsc::channel::<RawFrame>(4);
    let (packet_tx, mut packet_rx) = mpsc::channel::<EncodedPacket>(32);

    // Start frame capture
    let mut capture = FrameCapture::new(&config.display);
    capture.start(frame_tx)?;

    // Start encoder
    let (width, height, frame_rate) = config.display.get_dimensions()?;
    let encoder = VideoEncoder::new(config.encoder.clone(), width, height, frame_rate);
    encoder.start(frame_rx, packet_tx)?;

    info!("Capture + Encoding pipeline initialized");
    info!("Waiting for encoded packets... Press Ctrl+C to stop");

    // Setup shutdown handler
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    // Main loop - process encoded packets
    let mut packet_count = 0u64;
    let mut keyframe_count = 0u64;
    let mut last_report = std::time::Instant::now();
    let mut total_bytes = 0u64;

    loop {
        tokio::select! {
            Some(packet) = packet_rx.recv() => {

                packet_count += 1;
                total_bytes += packet.data.len() as u64;
                if packet.is_keyframe {
                    keyframe_count += 1;
                }

                // Report stats every second
                let now = std::time::Instant::now();
                let elapsed = now.duration_since(last_report);

                if elapsed.as_secs() >= 1 {
                    let pps = packet_count as f64 / elapsed.as_secs_f64();
                    let kbps = (total_bytes as f64 * 8.0) / (elapsed.as_secs_f64() * 1000.0);
                    let compression_ratio = if total_bytes > 0 {
                        ((width * height * 4) as f64 * pps) / total_bytes as f64
                    } else {
                        0.0
                    };

                    info!(
                        "Stats: {:.1} pps | {:.1} kbps | {}x{} | {:.1}x compression | {} keyframes",
                        pps,
                        kbps,
                        width,
                        height,
                        compression_ratio,
                        keyframe_count
                    );

                    packet_count = 0;
                    total_bytes = 0;
                    keyframe_count = 0;
                    last_report = now;
                }

                // TODO: Send to transport layer (Phase 3)
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
