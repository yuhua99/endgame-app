use std::sync::Arc;

use eg_shell::Shell;
use eg_studio::Studio;
use eg_transport::{Connection, DeviceInfo};

use crate::Result;

/// A connected trackball. Clones share the connection; it closes when the last clone drops.
#[derive(Clone)]
pub struct Session {
    pub name: String,
    /// e.g. "v1.6.2", or "unknown" if `board version` output is not recognized.
    pub firmware: String,
    shell: Shell,
    studio: Option<Studio>,
    _conn: Arc<Connection>,
}

impl Session {
    pub async fn connect(dev: &DeviceInfo) -> Result<Session> {
        let conn = eg_transport::open(dev).await?;
        let link = conn.shell().await?;
        let shell = Shell::open(link.tx, link.rx).await?;
        let firmware = parse_firmware(&shell.exec("board version").await?).unwrap_or_else(|| "unknown".into());
        // Keymap editing is optional: the connection is still useful without it.
        let studio = match conn.studio().await {
            Some(Ok(link)) => Some(Studio::open(link.tx, link.rx)),
            Some(Err(e)) => {
                log::warn!("studio channel unavailable: {e}");
                None
            }
            None => None,
        };
        Ok(Session { name: dev.name.clone(), firmware, shell, studio, _conn: Arc::new(conn) })
    }

    pub fn shell(&self) -> &Shell {
        &self.shell
    }

    /// ZMK Studio RPC, if the firmware exposes it on this connection.
    pub fn studio(&self) -> Option<&Studio> {
        self.studio.as_ref()
    }
}

/// `Firmware: v1.6.2` (observed) or `Firmware version: 1.6.2` (older, per Marshmellow UI).
fn parse_firmware(out: &str) -> Option<String> {
    out.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("Firmware")?;
        let (_, version) = rest.split_once(':')?;
        Some(version.trim().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_firmware() {
        let out = include_str!("../tests/fixtures/v1.6.2/board_version.txt");
        assert_eq!(parse_firmware(out).as_deref(), Some("v1.6.2"));
        assert_eq!(parse_firmware("Firmware version: 1.0.1").as_deref(), Some("1.0.1"));
        assert_eq!(parse_firmware("nope"), None);
    }
}
