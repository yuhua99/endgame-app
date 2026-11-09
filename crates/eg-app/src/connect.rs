use dioxus::prelude::*;
use eg_device::Session;
use eg_transport::DeviceInfo;

#[component]
pub fn ConnectPage(session: Signal<Option<Session>>) -> Element {
    let mut session = session;
    let mut devices = use_signal(Vec::<DeviceInfo>::new);
    let mut status = use_signal(String::new);
    let mut busy = use_signal(|| false);

    let scan = move |_| async move {
        busy.set(true);
        status.set("Scanning…".into());
        match eg_transport::scan().await {
            Ok(found) => {
                status.set(if found.is_empty() { "No trackball found.".into() } else { String::new() });
                devices.set(found);
            }
            Err(e) => status.set(format!("Scan failed: {e}")),
        }
        busy.set(false);
    };

    let connect = move |dev: DeviceInfo| async move {
        busy.set(true);
        status.set(format!("Connecting to {}…", dev.name));
        let result = Session::connect(&dev).await;
        busy.set(false);
        match result {
            Ok(s) => session.set(Some(s)),
            Err(e) => status.set(format!("Connection failed: {e}")),
        }
    };

    rsx! {
        div { class: "connect",
            h1 { "Endgame" }
            p { class: "hint",
                "Pair the trackball with this computer over Bluetooth and make sure it is connected, then scan."
            }
            button { disabled: busy(), onclick: scan, "Scan" }
            ul { class: "devices",
                for (i, dev) in devices.read().iter().cloned().enumerate() {
                    li { key: "{i}",
                        span { "{dev.name}" }
                        button { disabled: busy(), onclick: move |_| connect(dev.clone()), "Connect" }
                    }
                }
            }
            if !status.read().is_empty() {
                p { class: "status", "{status}" }
            }
        }
    }
}
