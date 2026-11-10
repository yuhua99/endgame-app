//! Byte links to an Endgame device. BLE only for now.

mod ble;

pub use ble::{open, scan, Connection, DeviceInfo};

use tokio::sync::mpsc;

/// Byte pipe to one device channel. `rx` ending means the channel closed.
/// `rx` is unbounded so incoming data is never dropped or blocks the link; callers must keep reading.
pub struct Link {
    pub tx: mpsc::Sender<Vec<u8>>,
    pub rx: mpsc::UnboundedReceiver<Vec<u8>>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("bluetooth: {0}")]
    Ble(#[from] bluest::Error),
    #[error("no bluetooth adapter")]
    NoAdapter,
    #[error("{0} not found on device")]
    NotFound(&'static str),
    #[error("timed out: {0}")]
    Timeout(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;
