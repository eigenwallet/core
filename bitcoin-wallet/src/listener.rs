use serde::{Deserialize, Serialize};

/// Progress information for Bitcoin wallet full scan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BitcoinFullScanProgress {
    Unknown,
    Known {
        current_index: u32,
        assumed_total: u32,
    },
}

/// Progress information for Bitcoin wallet sync
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BitcoinSyncProgress {
    Unknown,
    Known {
        consumed: u64,
        total: u64,
    },
}

/// Handle for tracking background processes
pub trait BackgroundProcessHandle: Send + Sync {
    /// Update the progress
    fn update(&self, progress: BitcoinFullScanProgress);
    
    /// Mark the process as finished
    fn finish(&self);
    
    /// Clone the handle
    fn clone_handle(&self) -> Box<dyn BackgroundProcessHandle>;
}

/// Handle for tracking sync processes
pub trait SyncProcessHandle: Send + Sync {
    /// Update the progress
    fn update(&self, progress: BitcoinSyncProgress);
    
    /// Mark the process as finished
    fn finish(&self);
    
    /// Clone the handle
    fn clone_handle(&self) -> Box<dyn SyncProcessHandle>;
}

/// Trait for listening to Bitcoin wallet events
pub trait BitcoinWalletListener: Send + Sync {
    /// Start a new full scan background process
    fn start_full_scan_process(&self) -> Box<dyn BackgroundProcessHandle>;
    
    /// Start a new sync background process
    fn start_sync_process(&self) -> Box<dyn SyncProcessHandle>;
}

/// No-op implementation for when no listener is needed
pub struct NoOpListener;

#[derive(Clone)]
struct NoOpBackgroundHandle;
impl BackgroundProcessHandle for NoOpBackgroundHandle {
    fn update(&self, _progress: BitcoinFullScanProgress) {}
    fn finish(&self) {}
    fn clone_handle(&self) -> Box<dyn BackgroundProcessHandle> {
        Box::new(NoOpBackgroundHandle)
    }
}

#[derive(Clone)]
struct NoOpSyncHandle;
impl SyncProcessHandle for NoOpSyncHandle {
    fn update(&self, _progress: BitcoinSyncProgress) {}
    fn finish(&self) {}
    fn clone_handle(&self) -> Box<dyn SyncProcessHandle> {
        Box::new(NoOpSyncHandle)
    }
}

impl BitcoinWalletListener for NoOpListener {
    fn start_full_scan_process(&self) -> Box<dyn BackgroundProcessHandle> {
        Box::new(NoOpBackgroundHandle)
    }
    
    fn start_sync_process(&self) -> Box<dyn SyncProcessHandle> {
        Box::new(NoOpSyncHandle)
    }
}