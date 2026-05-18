//! Re-export shared `liveblock-regions` types so the Linux port shares the
//! on-disk JSON format with macOS / Windows byte-for-byte.

pub use liveblock_regions::{NormalizedRegion, RegionStore};

use std::sync::Arc;

pub type SharedRegionStore = Arc<RegionStore>;
