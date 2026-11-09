//! Endgame device domain: session setup and feature APIs over the shell.
//! No UI types here; the app calls these and renders the results.

pub mod keymap;
pub mod rtcfg;
pub mod sensor;
mod session;

pub use session::Session;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Transport(#[from] eg_transport::Error),
    #[error(transparent)]
    Shell(#[from] eg_shell::Error),
    #[error(transparent)]
    Studio(#[from] eg_studio::Error),
    #[error("device rejected the request: {0}")]
    Rejected(String),
    #[error("unexpected output of {cmd:?}: {output:?}")]
    Parse { cmd: String, output: String },
}

impl Error {
    /// The device must be unlocked (Studio Unlock key) before keymap requests.
    pub fn is_unlock_required(&self) -> bool {
        matches!(self, Error::Studio(eg_studio::Error::UnlockRequired))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
