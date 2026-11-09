use dioxus::prelude::*;
use eg_device::Session;

use crate::advanced::AdvancedTab;
use crate::keymap::KeymapTab;
use crate::sensor::SensorTab;

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
    let Some(s) = session() else {
        return rsx! {};
    };

    rsx! {
        header { class: "topbar",
            span { class: "device", "{s.name}" }
            span { class: "firmware", "Firmware {s.firmware}" }
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
                div { key: "{t.title()}", hidden: tab() != t,
                    {
                        match t {
                            Tab::Sensor => rsx! { SensorTab { session } },
                            Tab::Advanced => rsx! { AdvancedTab { session } },
                            Tab::Keymap => rsx! { KeymapTab { session } },
                            _ => rsx! { p { class: "placeholder", "{t.title()} is not implemented yet." } },
                        }
                    }
                }
            }
        }
    }
}
