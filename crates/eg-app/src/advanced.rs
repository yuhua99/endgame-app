use dioxus::prelude::*;
use eg_device::rtcfg::{self, Param};
use eg_device::Session;

use crate::editable::use_editable;
use crate::widgets::{Actions, Number, Toggle};

/// All `rtcfg` parameters, grouped by module prefix.
#[component]
pub fn AdvancedTab(session: Signal<Option<Session>>, visible: ReadSignal<bool>) -> Element {
    let ed = use_editable::<Vec<Param>>();
    let shell = move || session.peek().as_ref().map(|s| s.shell().clone());
    let reload = move || async move {
        if let Some(shell) = shell() {
            ed.run(async move { rtcfg::list(&shell).await }).await;
        }
    };
    let apply = move || async move {
        if let (Some(shell), Some(saved), Some(new)) = (shell(), ed.saved.cloned(), ed.form.cloned()) {
            ed.run(async move { rtcfg::apply(&shell, &saved, &new).await }).await;
        }
    };
    let reset = move |key: String| async move {
        if let Some(shell) = shell() {
            ed.run(async move { rtcfg::reset(&shell, &key).await }).await;
        }
    };
    // Other tabs can change these values on the device, so read them again whenever this tab
    // opens, unless there are edits to keep.
    use_effect(move || {
        if visible() && ed.idle() {
            spawn(reload());
        }
    });

    let Some(params) = ed.form.cloned() else {
        return rsx! {
            if let Some(e) = ed.error.cloned() {
                p { class: "error", "{e}" }
                button { disabled: (ed.busy)(), onclick: move |_| reload(), "Retry" }
            } else {
                p { class: "placeholder", "Reading parameters…" }
            }
        };
    };
    let busy = (ed.busy)();
    let dirty = ed.dirty();
    // Reset is offered only where the device value differs from the default.
    let saved = ed.saved.cloned().unwrap_or_default();
    let differs = move |key: &str| saved.iter().any(|s| s.key == key && s.value != s.default);
    let mut groups: Vec<(String, Vec<Param>)> = Vec::new();
    for p in params {
        let module = p.key.split('/').next().unwrap_or_default().to_string();
        match groups.iter_mut().find(|(m, _)| *m == module) {
            Some((_, list)) => list.push(p),
            None => groups.push((module, vec![p])),
        }
    }

    rsx! {
        p { class: "placeholder", "Values are stored on the device. Reset restores a parameter's built-in default immediately." }
        div { class: "cards wide",
            for (module, list) in groups {
                section { class: "card", key: "{module}",
                    h2 { "{module}" }
                    for p in list {
                        div { class: "param", key: "{p.key}",
                            {param_row(p.clone(), busy, ed)}
                            span { class: "reset-slot",
                                if differs(&p.key) {
                                    button {
                                        class: "ghost small",
                                        title: "Reset to default ({p.default})",
                                        disabled: busy || dirty,
                                        onclick: {
                                            let key = p.key.clone();
                                            move |_| reset(key.clone())
                                        },
                                        "Reset"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Actions {
            error: ed.error.cloned(),
            busy,
            dirty,
            on_revert: move |_| ed.revert(),
            on_reload: move |_| {
                spawn(reload());
            },
            on_apply: move |_| {
                spawn(apply());
            },
        }
    }
}

fn param_row(p: Param, busy: bool, ed: crate::editable::Editable<Vec<Param>>) -> Element {
    let label = p.key.split_once('/').map_or(p.key.as_str(), |(_, rest)| rest).to_string();
    let key = p.key.clone();
    let set = move |v: i32| {
        let key = key.clone();
        ed.edit(move |list| {
            if let Some(p) = list.iter_mut().find(|p| p.key == key) {
                p.value = v;
            }
        })
    };
    if p.is_bool() {
        rsx! {
            Toggle { label, disabled: busy, checked: p.value == 1, on_change: move |v: bool| set(v as i32) }
        }
    } else {
        rsx! {
            Number {
                label,
                disabled: busy,
                value: p.value,
                min: p.min,
                max: p.max,
                hint: format!("{}–{}", p.min, p.max),
                on_change: move |v: i32| set(v),
            }
        }
    }
}
