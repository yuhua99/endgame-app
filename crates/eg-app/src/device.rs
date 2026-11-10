use std::time::Duration;

use dioxus::prelude::*;
use eg_device::board::{self, Battery, Output};
use eg_device::Session;

use crate::advanced::AdvancedTab;
use crate::keymap::KeymapTab;
use crate::sensor::SensorTab;

/// Pause between readings of the battery level and active output.
const STATUS_POLL: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Sensor,
    Advanced,
    Keymap,
    Lighting,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Sensor, Tab::Advanced, Tab::Keymap, Tab::Lighting];

    fn title(self) -> &'static str {
        match self {
            Tab::Sensor => "Sensor",
            Tab::Advanced => "Advanced",
            Tab::Keymap => "Keymap",
            Tab::Lighting => "Lighting",
        }
    }
}

#[component]
pub fn DevicePage(session: Signal<Option<Session>>) -> Element {
    let mut session = session;
    let mut tab = use_signal(|| Tab::Sensor);
    // A tab is mounted on first visit and then only hidden, so switching never cancels
    // a running operation and each tab loads its data once.
    let mut opened = use_signal(|| vec![Tab::Sensor]);
    let mut status = use_signal(board::Status::default);
    use_hook(move || {
        spawn(async move {
            loop {
                let Some(shell) = session.peek().as_ref().map(|s| s.shell().clone()) else {
                    break;
                };
                let new = board::status(&shell).await.unwrap_or_default();
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
                                Tab::Keymap => rsx! { KeymapTab { session } },
                                _ => rsx! { p { class: "placeholder", "{t.title()} is not implemented yet." } },
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
