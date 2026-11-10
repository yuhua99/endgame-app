//! Endgame device domain: session setup and feature APIs over the shell.
//! No UI types here; the app calls these and renders the results.

pub mod board;
pub mod keymap;
pub mod lighting;
pub mod profiles;
pub mod rtcfg;
pub mod sensor;
mod session;

pub use eg_shell::LogLine;
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
    #[error("the keymap has unsaved changes; save or discard them first")]
    UnsavedKeymap,
    #[error("unexpected output of {cmd:?}: {output:?}")]
    Parse { cmd: String, output: String },
}

impl Error {
    /// The device must be unlocked (Studio Unlock key) before keymap requests.
    pub fn is_unlock_required(&self) -> bool {
        matches!(self, Error::Studio(eg_studio::Error::UnlockRequired))
    }

    /// The firmware does not have the shell command.
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Error::Shell(eg_shell::Error::Unsupported(_)))
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Runs a command that reports a failure as an "Error..." line, as `rtcfg` does for an
/// unknown key, a value out of range, or a storage error.
pub(crate) async fn exec_checked(shell: &eg_shell::Shell, cmd: &str) -> Result<()> {
    let out = shell.exec(cmd).await?;
    match out.lines().find(|l| l.trim_start().starts_with("Error")) {
        Some(line) => Err(Error::Rejected(format!("{cmd}: {}", line.trim()))),
        None => Ok(()),
    }
}

/// Values of lines starting with `key`, e.g. "Pointer: 28%" -> "28%".
pub(crate) fn fields<'a>(out: &'a str, key: &'a str) -> impl Iterator<Item = &'a str> {
    out.lines().filter_map(move |line| line.trim().strip_prefix(key)).map(str::trim)
}

/// Output of a command that some firmware versions lack.
pub(crate) async fn exec_optional(shell: &eg_shell::Shell, cmd: &str) -> Result<Option<String>> {
    match shell.exec(cmd).await {
        Ok(out) => Ok(Some(out)),
        Err(eg_shell::Error::Unsupported(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
