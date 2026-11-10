//! Client for the Zephyr shell running on the trackball.
//!
//! Takes a plain byte channel pair, so it works over any transport and tests need no hardware.

mod client;
mod framer;

pub use client::{LogLine, Shell};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no response to {0:?}")]
    Timeout(String),
    #[error("device does not support {0:?}")]
    Unsupported(String),
    #[error("link closed")]
    Closed,
}

pub type Result<T> = std::result::Result<T, Error>;
