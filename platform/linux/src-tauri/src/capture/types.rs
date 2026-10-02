use anyhow::Result;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct FrameView {
    /// Opaque BGRA8 pixel data, row-major, no padding.
    pub pixels: Arc<[u8]>,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

#[async_trait::async_trait]
pub trait CaptureSource: Send {
    async fn next_frame(&mut self) -> Result<FrameView>;
    fn stop(&mut self);
}
