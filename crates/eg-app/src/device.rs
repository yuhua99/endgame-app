use std::time::Duration;

use dioxus::prelude::*;
use eg_device::board::{self, Battery, Output};
use eg_device::Session;

use crate::advanced::AdvancedTab;
use crate::keymap::KeymapTab;
use crate::lighting::LightingTab;
use crate::logs::{self, LogsTab};
use crate::profiles::ProfilesTab;
use crate::sensor::SensorTab;

/// Pause between readings of the battery level and active output.
const STATUS_POLL: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Sensor,
    Advanced,
    Keymap,
    Profiles,
    Lighting,
    Logs,
}

impl Tab {
    const ALL: [Tab; 6] = [Tab::Sensor, Tab::Advanced, Tab::Keymap, Tab::Profiles, Tab::Lighting, Tab::Logs];

    fn title(self) -> &'static str {
        match self {
            Tab::Sensor => "Sensor",
            Tab::Advanced => "Advanced",
            Tab::Keymap => "Keymap",
            Tab::Profiles => "Profiles",
            Tab::Lighting => "Lighting",
            Tab::Logs => "Logs",
        }
    }
}

#[component]
pub fn DevicePage(session: Signal<Option<Session>>, connect_status: Signal<String>) -> Element {
    let mut session = session;
    let mut connect_status = connect_status;
    let mut tab = use_signal(|| Tab::Sensor);
    // A tab is mounted on first visit and then only hidden, so switching never cancels
    // a running operation and each tab loads its data once.
    let mut opened = use_signal(|| vec![Tab::Sensor]);
    let mut status = use_signal(board::Status::default);
    // Bumped by the Profiles tab when it replaces the keymap, so the Keymap tab reads it again.
    let keymap_changed = use_signal(|| 0u32);
    // Collected here rather than in the Logs tab, which is mounted only when first opened.
    let log = logs::use_log(session);
    use_hook(move || {
        spawn(async move {
            loop {
                let Some(shell) = session.peek().as_ref().map(|s| s.shell().clone()) else {
                    break;
                };
                // This poll doubles as the heartbeat: no answer means the connection is gone.
                let new = match board::status(&shell).await {
                    Ok(new) => new,
                    // The firmware answered, it just lacks the command.
                    Err(e) if e.is_unsupported() => board::Status::default(),
                    Err(e) => {
                        connect_status.set(format!("Connection lost: {e}"));
                        session.set(None);
                        break;
                    }
                };
                if *status.peek() != new {
                    status.set(new);
                }
                tokio::time::sleep(STATUS_POLL).await;
            }
        });
    });
    let Some(s) = session() else {
        return rsx! {};
    };

    rsx! {
        div { class: "shell",
            header { class: "topbar",
                span { class: "device", "{s.name}" }
                span { class: "firmware", "Firmware {s.firmware}" }
                if let Some(output) = status().output {
                    span { class: "output", title: "Where the trackball sends its input", "Output: {output_label(output)}" }
                }
                if let Some(battery) = status().battery {
                    BatteryLevel { battery }
                }
                button { class: "ghost", onclick: move |_| session.set(None), "Disconnect" }
            }
            nav { class: "tabs",
                for t in Tab::ALL {
                    button {
                        key: "{t.title()}",
                        class: if tab() == t { "tab active" } else { "tab" },
                        onclick: move |_| {
                            tab.set(t);
                            if !opened.read().contains(&t) {
                                opened.write().push(t);
                            }
                        },
                        "{t.title()}"
                    }
                }
            }
            main { class: "content",
                for t in opened() {
                    div { key: "{t.title()}", class: "pane", hidden: tab() != t,
                        {
                            match t {
                                Tab::Sensor => rsx! { SensorTab { session, visible: tab() == t } },
                                Tab::Advanced => rsx! { AdvancedTab { session, visible: tab() == t } },
                                Tab::Keymap => rsx! { KeymapTab { session, reload: keymap_changed } },
                                Tab::Profiles => rsx! { ProfilesTab { session, visible: tab() == t, keymap_changed } },
                                Tab::Lighting => rsx! { LightingTab { session, visible: tab() == t } },
                                Tab::Logs => rsx! { LogsTab { session, log } },
                            }
                        }
                    }
                }
            }
        }
    }
}

fn output_label(output: Output) -> &'static str {
    match output {
        Output::Usb => "USB",
        Output::Ble => "Bluetooth",
        Output::Dongle => "Dongle",
    }
}

#[component]
fn BatteryLevel(battery: Battery) -> Element {
    // Marshmellow UI's colour thresholds.
    let level = match battery.percent {
        0..=15 => "low",
        16..=30 => "mid",
        _ => "high",
    };
    let title = if battery.usb_powered { "Battery (not accurate while USB power is present)" } else { "Battery" };
    rsx! {
        span { class: "battery", title,
            div { class: "meter",
                div { class: level, width: "{battery.percent.min(100)}%" }
            }
            "{battery.percent}%"
        }
    }
}
