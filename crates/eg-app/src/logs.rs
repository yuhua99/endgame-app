//! Shell traffic: the commands the app sent and what the trackball answered, with a
//! field for sending a command by hand. Polled readings are left out.

use std::time::{Duration, Instant};

use dioxus::prelude::*;
use eg_device::{LogLine, Session};
use tokio::sync::broadcast::error::RecvError;

use crate::widgets::ActionBar;

#[derive(Clone, PartialEq)]
pub struct Entry {
    /// Time since the connection was opened.
    at: Duration,
    line: LogLine,
}

/// Collects the session's shell traffic from now on. Call it before any tab runs a command.
pub fn use_log(session: Signal<Option<Session>>) -> Signal<Vec<Entry>> {
    let mut log = use_signal(Vec::new);
    use_hook(move || {
        let Some(mut lines) = session.peek().as_ref().map(|s| s.shell().subscribe()) else {
            return;
        };
        let start = Instant::now();
        spawn(async move {
            loop {
                match lines.recv().await {
                    // A command without output has nothing to show.
                    Ok(LogLine::Received(out)) if out.is_empty() => {}
                    Ok(line) => log.write().push(Entry { at: start.elapsed(), line }),
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
        });
    });
    log
}

#[component]
pub fn LogsTab(session: Signal<Option<Session>>, log: Signal<Vec<Entry>>) -> Element {
    let mut log = log;
    let mut command = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let empty = log.read().is_empty();

    rsx! {
        // Reversed column: the view stays at the newest line as lines are added.
        div { class: "card log",
            div {
                for (i, e) in log.read().iter().enumerate() {
                    LogRow { key: "{i}", entry: e.clone() }
                }
            }
        }
        ActionBar { error: error(),
            input {
                class: "command",
                r#type: "text",
                placeholder: "Shell command",
                value: "{command}",
                oninput: move |e| command.set(e.value()),
                onkeydown: move |e| {
                    if e.key() == Key::Enter {
                        send(session, command);
                    }
                },
            }
            button { disabled: command.read().trim().is_empty(), onclick: move |_| send(session, command), "Send" }
            button { class: "ghost", disabled: empty, onclick: move |_| log.write().clear(), "Clear" }
            button {
                class: "ghost",
                disabled: empty,
                onclick: move |_| {
                    let text = export_text(&log.peek());
                    spawn(async move {
                        let dialog = rfd::AsyncFileDialog::new().set_file_name("endgame-log.txt");
                        let Some(file) = dialog.save_file().await else {
                            return;
                        };
                        let saved = file.write(text.as_bytes()).await;
                        error.set(saved.err().map(|e| format!("Could not save the log: {e}")));
                    });
                },
                "Export"
            }
        }
    }
}

#[component]
fn LogRow(entry: Entry) -> Element {
    let (class, mark, text) = parts(&entry.line);
    rsx! {
        div { class: "line {class}",
            span { class: "time", "{time(entry.at)}" }
            span { class: "mark", "{mark}" }
            span { class: "text", "{text}" }
        }
    }
}

/// Runs the typed command; the log shows its output or failure.
fn send(session: Signal<Option<Session>>, mut command: Signal<String>) {
    let cmd = command.peek().trim().to_string();
    let Some(shell) = session.peek().as_ref().map(|s| s.shell().clone()) else {
        return;
    };
    if cmd.is_empty() {
        return;
    }
    command.set(String::new());
    spawn(async move {
        let _ = shell.exec(&cmd).await;
    });
}

/// CSS class, direction mark, and text of a line.
fn parts(line: &LogLine) -> (&'static str, &'static str, &str) {
    match line {
        LogLine::Sent(cmd) => ("sent", ">", cmd),
        LogLine::Received(out) => ("received", "<", out),
        LogLine::Failed(e) => ("failed", "!", e),
    }
}

/// Minutes, seconds, and milliseconds, as in "02:05.030".
fn time(at: Duration) -> String {
    let ms = at.as_millis();
    format!("{:02}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

fn export_text(log: &[Entry]) -> String {
    log.iter().map(|e| format!("[{}] {} {}\n", time(e.at), parts(&e.line).1, parts(&e.line).2)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_one_entry_per_line() {
        let log = [
            Entry { at: Duration::from_millis(125_030), line: LogLine::Sent("rrl get".into()) },
            Entry { at: Duration::from_millis(125_090), line: LogLine::Received("8 ms".into()) },
            Entry { at: Duration::from_secs(3600), line: LogLine::Failed("link closed".into()) },
        ];
        assert_eq!(export_text(&log), "[02:05.030] > rrl get\n[02:05.090] < 8 ms\n[60:00.000] ! link closed\n");
    }
}
