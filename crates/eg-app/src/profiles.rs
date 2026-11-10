use dioxus::prelude::*;
use eg_device::profiles::{self, Change, Os, Profiles, Slot};
use eg_device::Session;

use crate::widgets::{ActionBar, Toggle};

const LOSS: &str = "The current keymap is not stored in a profile and will be lost.";

/// Keymap profiles, the profile each output activates, and the OS mode.
/// `keymap_changed` is bumped when a change here replaces the keymap.
#[component]
pub fn ProfilesTab(
    session: Signal<Option<Session>>,
    visible: ReadSignal<bool>,
    keymap_changed: Signal<u32>,
) -> Element {
    let mut keymap_changed = keymap_changed;
    let mut data = use_signal(|| None::<Profiles>);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    // A change that waits for the user's confirmation, with the question to ask.
    let mut confirm = use_signal(|| None::<(String, Change)>);

    let mut finish = move |result: eg_device::Result<Profiles>| {
        match result {
            Ok(p) => {
                data.set(Some(p));
                error.set(None);
            }
            Err(e) => error.set(Some(e.to_string())),
        }
        busy.set(false);
    };
    let reload = move || async move {
        let Some(s) = session.peek().clone() else { return };
        if *busy.peek() {
            return;
        }
        busy.set(true);
        finish(profiles::read(s.shell()).await);
    };
    let apply = move |change: Change| async move {
        let Some(s) = session.peek().clone() else { return };
        confirm.set(None);
        busy.set(true);
        match profiles::change(&s, &change).await {
            Ok(()) => {
                if matches!(change, Change::Activate(_) | Change::RestoreDefault) {
                    *keymap_changed.write() += 1;
                }
                finish(profiles::read(s.shell()).await);
            }
            Err(e) => finish(Err(e)),
        }
    };
    // Applies a change, after a confirmation if it comes with a question.
    let mut request = move |(question, change): (Option<String>, Change)| match question {
        Some(q) => {
            error.set(None);
            confirm.set(Some((q, change)));
        }
        None => {
            spawn(apply(change));
        }
    };
    // Other tabs change what is shown here (such as saving in the Keymap tab), so read
    // again whenever this tab opens.
    use_effect(move || {
        if visible() {
            spawn(reload());
        }
    });

    let Some(p) = data() else {
        return rsx! {
            if let Some(e) = error() {
                p { class: "error", "{e}" }
                button { disabled: busy(), onclick: move |_| reload(), "Retry" }
            } else {
                p { class: "placeholder", "Reading profiles…" }
            }
        };
    };
    let busy_now = busy();
    // Replacing the keymap now would lose it.
    let unstored = p.customized && !p.slots.iter().any(|s| s.active);
    // Outputs are bound by name and the firmware takes the first slot with it, so a name
    // is listed once and cannot be given to a second profile.
    let mut named: Vec<(u32, String)> = Vec::new();
    for s in &p.slots {
        if let Some(name) = s.name.clone().filter(|n| !n.is_empty() && !named.iter().any(|(_, m)| m == n)) {
            named.push((s.index, name));
        }
    }
    let taken: Vec<String> = named.iter().map(|(_, name)| name.clone()).collect();

    rsx! {
        p { class: "placeholder",
            "A profile stores the changes made in the Keymap tab together with the OS mode."
        }
        section { class: "card",
            h2 { "Profiles" }
            div { class: "slot",
                span { class: "label", "Default" }
                span { class: "name",
                    "Firmware keymap"
                    if !p.customized {
                        span { class: "badge", "Active" }
                    }
                }
                button {
                    class: "ghost",
                    disabled: busy_now || !p.customized,
                    onclick: move |_| {
                        request((unstored.then(|| format!("{LOSS} Restore the default?")), Change::RestoreDefault))
                    },
                    "Restore"
                }
            }
            for slot in p.slots.clone() {
                SlotRow {
                    key: "{slot.index}",
                    slot,
                    disabled: busy_now,
                    customized: p.customized,
                    unstored,
                    taken: taken.clone(),
                    on_request: request,
                }
            }
        }
        if !p.assignments.is_empty() {
            section { class: "card",
                h2 { "Outputs" }
                if let Some(on) = p.autoswitch {
                    Toggle {
                        label: "Switch automatically",
                        disabled: busy_now,
                        checked: on,
                        on_change: move |v: bool| request((None, Change::Autoswitch(v))),
                    }
                }
                for a in p.assignments.clone() {
                    label { key: "{a.output}", class: "row",
                        span { class: "label", "{output_label(&a.output)}" }
                        select {
                            disabled: busy_now,
                            onchange: {
                                let output = a.output.clone();
                                move |e: FormEvent| {
                                    let slot = e.value().parse().ok();
                                    request((None, Change::Assign { output: output.clone(), slot }))
                                }
                            },
                            option { value: "", selected: a.profile.is_none(), "None" }
                            for (index, name) in named.clone() {
                                option {
                                    key: "{index}",
                                    value: "{index}",
                                    selected: a.profile.as_ref() == Some(&name),
                                    "{name}"
                                }
                            }
                            if let Some(name) = a.profile.clone().filter(|n| !named.iter().any(|(_, m)| m == n)) {
                                option { value: "", selected: true, disabled: true, "{name} (missing)" }
                            }
                        }
                    }
                }
            }
        }
        if p.os.is_some() || p.default_os.is_some() {
            section { class: "card",
                h2 { "OS mode" }
                if let Some(os) = p.os {
                    OsRow {
                        label: "Current",
                        disabled: busy_now,
                        os,
                        on_change: move |os: Os| request((None, Change::Os(os))),
                    }
                }
                if let Some(os) = p.default_os {
                    OsRow {
                        label: "Default",
                        disabled: busy_now,
                        os,
                        on_change: move |os: Os| request((None, Change::DefaultOs(os))),
                    }
                }
            }
        }
        ActionBar { error: error(),
            if let Some((question, change)) = confirm() {
                span { class: "prompt", "{question}" }
                button { class: "ghost", onclick: move |_| confirm.set(None), "Cancel" }
                button {
                    disabled: busy_now,
                    onclick: move |_| {
                        spawn(apply(change.clone()));
                    },
                    "Confirm"
                }
            } else {
                button {
                    class: "ghost",
                    disabled: busy_now,
                    onclick: move |_| {
                        spawn(reload());
                    },
                    "Reload"
                }
            }
        }
    }
}

/// "usb" and "wireless-N", named as in Marshmellow UI.
fn output_label(output: &str) -> String {
    match output.strip_prefix("wireless-") {
        None => "USB".into(),
        Some("6") => "Dongle".into(),
        Some(n) => format!("Bluetooth {n}"),
    }
}

/// One slot: its profile with the actions on it, or a name field to store the keymap there.
#[component]
fn SlotRow(
    slot: Slot,
    disabled: bool,
    customized: bool,
    unstored: bool,
    /// Names other profiles already have.
    taken: Vec<String>,
    on_request: EventHandler<(Option<String>, Change)>,
) -> Element {
    let mut new_name = use_signal(String::new);
    let index = slot.index;
    let name_max = profiles::NAME_MAX;
    let can_save = customized && profiles::valid_name(&new_name.read()) && !taken.contains(&*new_name.read());
    rsx! {
        div { class: "slot",
            span { class: "label", "Slot {index}" }
            {
                match slot.name.clone() {
                    None => rsx! {
                        span { class: "name",
                            input {
                                r#type: "text",
                                placeholder: "Profile name",
                                title: "Letters, digits, - and _; at most {name_max} characters; not used by another profile",
                                maxlength: "{name_max}",
                                disabled,
                                value: "{new_name}",
                                oninput: move |e| new_name.set(e.value()),
                            }
                        }
                        button {
                            disabled: disabled || !can_save,
                            onclick: move |_| on_request.call((None, Change::Save { slot: index, name: new_name() })),
                            "Save current"
                        }
                    },
                    Some(name) => {
                        let shown = if name.is_empty() { "(unnamed)".to_string() } else { format!("\"{name}\"") };
                        let activate = unstored.then(|| format!("{LOSS} Activate {shown}?"));
                        let overwrite = format!("Replace profile {shown} in slot {index} with the current keymap?");
                        let delete = format!("Delete profile {shown} from slot {index}?");
                        rsx! {
                            span { class: "name",
                                if name.is_empty() { "(unnamed)" } else { "{name}" }
                                if slot.active {
                                    span { class: "badge", "Active" }
                                }
                            }
                            button {
                                class: "ghost",
                                disabled: disabled || slot.active,
                                onclick: move |_| on_request.call((activate.clone(), Change::Activate(index))),
                                "Activate"
                            }
                            button {
                                class: "ghost",
                                disabled: disabled || slot.active || !customized || name.is_empty(),
                                onclick: {
                                    let name = name.clone();
                                    move |_| {
                                        let change = Change::Overwrite { slot: index, name: name.clone() };
                                        on_request.call((Some(overwrite.clone()), change))
                                    }
                                },
                                "Overwrite"
                            }
                            button {
                                class: "ghost",
                                disabled,
                                onclick: move |_| on_request.call((Some(delete.clone()), Change::Delete(index))),
                                "Delete"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn OsRow(#[props(into)] label: String, disabled: bool, os: Os, on_change: EventHandler<Os>) -> Element {
    rsx! {
        label { class: "row",
            span { class: "label", "{label}" }
            select {
                disabled,
                onchange: move |e| on_change.call(if e.value() == "mac" { Os::Mac } else { Os::WinLinux }),
                option { value: "winlinux", selected: os == Os::WinLinux, "Windows / Linux" }
                option { value: "mac", selected: os == Os::Mac, "macOS" }
            }
        }
    }
}
