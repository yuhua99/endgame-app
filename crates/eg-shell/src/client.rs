use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Mutex};

use crate::framer::{strip_logs, Framer};
use crate::{Error, Result};

/// A command fails with `Error::Timeout` when its prompt has not arrived within this time,
/// even if log output keeps arriving.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Zephyr shell client. Clones share one session; commands run one at a time in call order.
#[derive(Clone)]
pub struct Shell {
    inner: Arc<Mutex<Inner>>,
}

impl Shell {
    /// `tx` carries bytes to the device, `rx` bytes from it (e.g. `eg_transport::Link`).
    pub async fn open(tx: mpsc::Sender<Vec<u8>>, rx: mpsc::UnboundedReceiver<Vec<u8>>) -> Result<Shell> {
        let mut inner = Inner { tx, rx, framer: Framer::default(), sync_id: 0, in_sync: false };
        inner.exec("shell echo off").await?;
        Ok(Shell { inner: Arc::new(Mutex::new(inner)) })
    }

    /// Runs `cmd` and returns its output without prompt, ANSI codes, CRs, or firmware log lines.
    /// Fails with `Unsupported` if the firmware does not know the command.
    pub async fn exec(&self, cmd: &str) -> Result<String> {
        self.inner.lock().await.exec(cmd).await
    }
}

struct Inner {
    tx: mpsc::Sender<Vec<u8>>,
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    framer: Framer,
    sync_id: u32,
    /// False until the next response is known to belong to the next command:
    /// at start, and after a command failed or was cancelled midway.
    in_sync: bool,
}

impl Inner {
    async fn exec(&mut self, cmd: &str) -> Result<String> {
        if !self.in_sync {
            self.sync().await?;
        }
        self.in_sync = false;
        self.send(cmd).await?;
        let out = self.recv(cmd).await?;
        self.in_sync = true;
        if out.contains("command not found") {
            return Err(Error::Unsupported(cmd.to_string()));
        }
        Ok(out)
    }

    /// Sends a unique unknown command and discards every response up to its
    /// "command not found" reply: greeting prompts and late output of earlier commands.
    /// Output that arrives after this reply is still misattributed.
    async fn sync(&mut self) -> Result<()> {
        self.sync_id += 1;
        let marker = format!("__sync{}", self.sync_id);
        let reply = format!("{marker}: command not found");
        self.send(&marker).await?;
        while !self.recv(&marker).await?.contains(&reply) {}
        self.in_sync = true;
        Ok(())
    }

    async fn send(&mut self, cmd: &str) -> Result<()> {
        self.tx.send(format!("{cmd}\n").into_bytes()).await.map_err(|_| Error::Closed)
    }

    async fn recv(&mut self, cmd: &str) -> Result<String> {
        let deadline = tokio::time::Instant::now() + TIMEOUT;
        loop {
            if let Some(out) = self.framer.pop() {
                let (out, had_logs) = strip_logs(&out);
                // A log printed while idle or mid-command is followed by a redrawn prompt;
                // that log-only chunk is not the response.
                if had_logs && out.is_empty() {
                    continue;
                }
                return Ok(out);
            }
            match tokio::time::timeout_at(deadline, self.rx.recv()).await {
                Ok(Some(bytes)) => self.framer.push(&bytes),
                Ok(None) => return Err(Error::Closed),
                Err(_) => return Err(Error::Timeout(cmd.to_string())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type DeviceTx = mpsc::UnboundedSender<Vec<u8>>;

    fn reply(body: &str) -> Vec<u8> {
        format!("\r\n{body}\r\n\r\n\x1b[1;32mendgame$ \x1b[0m").into_bytes()
    }

    /// Fake device: sends a greeting prompt, then answers each command line via `handle`.
    fn device(handle: fn(&str, &DeviceTx)) -> (mpsc::Sender<Vec<u8>>, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (tx, mut cmds) = mpsc::channel::<Vec<u8>>(16);
        let (out, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let _ = out.send(b"\x1b[1;32mendgame$ \x1b[0m".to_vec());
            while let Some(line) = cmds.recv().await {
                let line = String::from_utf8(line).unwrap();
                handle(line.trim_end_matches('\n'), &out);
            }
        });
        (tx, rx)
    }

    fn endgame(cmd: &str, out: &DeviceTx) {
        match cmd {
            "shell echo off" => drop(out.send(reply(""))),
            "board version" => drop(out.send(reply("Firmware: v1.6.2"))),
            "logged" => {
                drop(out.send(reply("[41468053] <wrn> zmk_studio: Skipping on")));
                drop(out.send(reply("[39874492] <inf> zmk_studio: RPC Notifications enabled\r\n\r\n280 (28%)")));
            }
            "chatty" => {
                let out = out.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        if out.send(b"<inf> log line\r\n".to_vec()).is_err() {
                            break;
                        }
                    }
                });
            }
            "slow" => {
                let out = out.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(TIMEOUT + Duration::from_millis(500)).await;
                    let _ = out.send(reply("late"));
                });
            }
            other => drop(out.send(reply(&format!("{other}: command not found")))),
        }
    }

    #[tokio::test]
    async fn exec_returns_clean_output_after_greeting() {
        let (tx, rx) = device(endgame);
        let shell = Shell::open(tx, rx).await.unwrap();
        assert_eq!(shell.exec("board version").await.unwrap(), "Firmware: v1.6.2");
    }

    #[tokio::test]
    async fn log_lines_and_log_only_prompts_are_skipped() {
        let (tx, rx) = device(endgame);
        let shell = Shell::open(tx, rx).await.unwrap();
        assert_eq!(shell.exec("logged").await.unwrap(), "280 (28%)");
        assert_eq!(shell.exec("board version").await.unwrap(), "Firmware: v1.6.2");
    }

    #[tokio::test]
    async fn unknown_command_is_unsupported() {
        let (tx, rx) = device(endgame);
        let shell = Shell::open(tx, rx).await.unwrap();
        assert!(matches!(shell.exec("argb evt").await, Err(Error::Unsupported(c)) if c == "argb evt"));
        assert_eq!(shell.exec("board version").await.unwrap(), "Firmware: v1.6.2");
    }

    #[tokio::test(start_paused = true)]
    async fn late_output_after_timeout_is_discarded() {
        let (tx, rx) = device(endgame);
        let shell = Shell::open(tx, rx).await.unwrap();
        assert!(matches!(shell.exec("slow").await, Err(Error::Timeout(c)) if c == "slow"));
        tokio::time::sleep(Duration::from_secs(1)).await; // "late" arrives in the meantime
        assert_eq!(shell.exec("board version").await.unwrap(), "Firmware: v1.6.2");
    }

    #[tokio::test(start_paused = true)]
    async fn steady_logs_do_not_extend_timeout() {
        let (tx, rx) = device(endgame);
        let shell = Shell::open(tx, rx).await.unwrap();
        assert!(matches!(shell.exec("chatty").await, Err(Error::Timeout(c)) if c == "chatty"));
    }

    #[tokio::test]
    async fn closed_link_fails() {
        let (tx, _cmds) = mpsc::channel(1);
        let (out, rx) = mpsc::unbounded_channel::<Vec<u8>>();
        drop(out);
        assert!(matches!(Shell::open(tx, rx).await, Err(Error::Closed)));
    }
}
