//! Re-export the shared `liveblock-regions` types so the Windows port shares
//! the on-disk format with macOS / Linux byte-for-byte.

pub use liveblock_regions::{NormalizedRegion, RegionStore};

use std::sync::Arc;

pub type SharedRegionStore = Arc<RegionStore>;
