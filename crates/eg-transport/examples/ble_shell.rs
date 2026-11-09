//! Spike: find the trackball over BLE, run shell commands, record raw responses.
//!
//!     ble_shell [--device N] ["cmd" ...]
//!
//! Without commands it runs DEFAULT_CMDS and waits for Enter before exiting, so it can be
//! double-clicked. Raw response bytes go to out.txt in the working directory; logs go to stderr.

use std::error::Error;
use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

/// Full colored prompt including the color reset, so each record ends complete.
const PROMPT: &str = "\x1b[1;32mendgame$ \x1b[0m";
const SYNC: &str = "__sync";
const IDLE: Duration = Duration::from_secs(3);
const OUT_FILE: &str = "out.txt";
const DEFAULT_CMDS: &[&str] = &[
    "board version",
    "p2sm status",
    "p2sm sens pointer get",
    "rtcfg list",
    "help",
    "p2sm --help",
    "rrl --help",
    "plane --help",
];

#[tokio::main]
async fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("eg_transport=debug,info")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let double_clicked = args.is_empty();
    let result = run(args).await;
    if let Err(e) = &result {
        eprintln!("error: {e}");
    }
    if double_clicked {
        eprintln!("press Enter to exit");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

async fn run(mut args: Vec<String>) -> Result<(), Box<dyn Error>> {
    let pick: usize = if args.first().is_some_and(|a| a == "--device") {
        args.remove(0);
        args.remove(0).parse()?
    } else {
        0
    };
    let cmds = if args.is_empty() {
        DEFAULT_CMDS.iter().map(|c| c.to_string()).collect()
    } else {
        args
    };

    let devices = eg_transport::scan().await?;
    for (i, d) in devices.iter().enumerate() {
        eprintln!("[{i}] {}", d.name);
    }
    let dev = devices.get(pick).ok_or("no device found")?;
    eprintln!("using [{pick}] {}", dev.name);

    let conn = eg_transport::open(dev).await?;
    let mut rec = Recorder { link: conn.shell().await?, buf: Vec::new() };

    // Skip the greeting prompt: everything up to the reply to an unknown command is discarded.
    rec.link.tx.send(format!("{SYNC}\n").into_bytes()).await?;
    loop {
        let (raw, status) = rec.read_until(PROMPT.as_bytes()).await;
        status?;
        if String::from_utf8_lossy(&raw).contains(&format!("{SYNC}: command not found")) {
            break;
        }
    }

    let mut file = std::fs::File::create(OUT_FILE)?;
    for cmd in std::iter::once("shell echo off".to_string()).chain(cmds) {
        rec.link.tx.send(format!("{cmd}\n").into_bytes()).await?;
        let (raw, status) = rec.read_until(PROMPT.as_bytes()).await;
        file.write_all(format!("===== {cmd}\n").as_bytes())?;
        file.write_all(&raw)?;
        file.write_all(b"\n")?;
        println!("===== {cmd}\n{}", String::from_utf8_lossy(&raw));
        status?;
    }
    eprintln!("raw output written to {OUT_FILE}");
    Ok(())
}

struct Recorder {
    link: eg_transport::Link,
    /// Bytes received after the last returned record.
    buf: Vec<u8>,
}

impl Recorder {
    /// Returns the bytes through the next `end` and keeps the rest for the next call.
    /// On failure returns whatever was received.
    async fn read_until(&mut self, end: &[u8]) -> (Vec<u8>, Result<(), String>) {
        loop {
            if let Some(at) = self.buf.windows(end.len()).position(|w| w == end) {
                let rest = self.buf.split_off(at + end.len());
                return (std::mem::replace(&mut self.buf, rest), Ok(()));
            }
            match tokio::time::timeout(IDLE, self.link.rx.recv()).await {
                Ok(Some(bytes)) => self.buf.extend_from_slice(&bytes),
                Ok(None) => return (std::mem::take(&mut self.buf), Err("link closed".to_string())),
                // Stop here: late output would be attributed to the next command.
                Err(_) => return (std::mem::take(&mut self.buf), Err(format!("no prompt within {IDLE:?}"))),
            }
        }
    }
}
