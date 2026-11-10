use dioxus::prelude::*;
use eg_device::Session;
use eg_transport::DeviceInfo;
use tokio::sync::mpsc;

#[component]
pub fn ConnectPage(session: Signal<Option<Session>>, status: Signal<String>) -> Element {
    let mut session = session;
    let mut status = status;
    let mut devices = use_signal(Vec::<DeviceInfo>::new);
    let mut busy = use_signal(|| false);
    let mut scanning = use_signal(|| false);

    // Devices are listed as they are found, and can be connected to while the scan runs.
    let scan = move |_| async move {
        scanning.set(true);
        status.set(String::new());
        devices.write().clear();
        let (tx, mut found) = mpsc::unbounded_channel();
        let list = async move {
            while let Some(device) = found.recv().await {
                devices.write().push(device);
            }
        };
        let (result, ()) = tokio::join!(eg_transport::scan(tx), list);
        // A connection attempt started during the scan owns the status line.
        if status.read().is_empty() {
            match result {
                Ok(()) if devices.read().is_empty() => status.set("No trackball found.".into()),
                Ok(()) => {}
                Err(e) => status.set(format!("Scan failed: {e}")),
            }
        }
        scanning.set(false);
    };

    let connect = move |dev: DeviceInfo| async move {
        busy.set(true);
        status.set(format!("Connecting to {}…", dev.name));
        let result = Session::connect(&dev).await;
        busy.set(false);
        match result {
            Ok(s) => {
                status.set(String::new());
                session.set(Some(s));
            }
            Err(e) => status.set(format!("Connection failed: {e}")),
        }
    };

    rsx! {
        div { class: "connect",
            h1 { "Endgame" }
            p { class: "hint",
                "Pair the trackball with this computer over Bluetooth and make sure it is connected, then scan."
            }
            button { disabled: busy() || scanning(), onclick: scan, "Scan" }
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
            } else if scanning() {
                p { class: "status", "Scanning…" }
            }
        }
    }
}
