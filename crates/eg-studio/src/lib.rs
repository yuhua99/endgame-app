//! ZMK Studio RPC client: framed protobuf over a byte channel pair.

mod client;
mod framing;

pub use client::Studio;

/// Types generated from `proto/zmk/*.proto`.
pub mod proto {
    pub mod studio {
        include!(concat!(env!("OUT_DIR"), "/zmk.studio.rs"));
    }
    pub mod meta {
        include!(concat!(env!("OUT_DIR"), "/zmk.meta.rs"));
    }
    pub mod core {
        include!(concat!(env!("OUT_DIR"), "/zmk.core.rs"));
    }
    pub mod behaviors {
        include!(concat!(env!("OUT_DIR"), "/zmk.behaviors.rs"));
    }
    pub mod keymap {
        include!(concat!(env!("OUT_DIR"), "/zmk.keymap.rs"));
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("device is locked; press the Studio unlock key on the trackball")]
    UnlockRequired,
    #[error("studio error: {0:?}")]
    Rpc(proto::meta::ErrorConditions),
    #[error("no response to studio request")]
    Timeout,
    #[error("studio response did not match the request")]
    UnexpectedResponse,
    #[error("studio link closed")]
    Closed,
}

pub type Result<T> = std::result::Result<T, Error>;
