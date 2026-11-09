use dioxus::prelude::*;
use eg_device::sensor::{self, SensorSettings};
use eg_device::Session;

use crate::editable::use_editable;
use crate::widgets::{Actions, Slider, Toggle};

#[component]
pub fn SensorTab(session: Signal<Option<Session>>) -> Element {
    let ed = use_editable::<SensorSettings>();
    let shell = move || session.peek().as_ref().map(|s| s.shell().clone());
    let reload = move || async move {
        if let Some(shell) = shell() {
            ed.run(async move { sensor::read(&shell).await }).await;
        }
    };
    let apply = move || async move {
        if let (Some(shell), Some(new)) = (shell(), ed.form.cloned()) {
            ed.run(async move { sensor::apply(&shell, &new).await }).await;
        }
    };
    use_hook(move || {
        spawn(reload());
    });

    let Some(f) = ed.form.cloned() else {
        return rsx! {
            if let Some(e) = ed.error.cloned() {
                p { class: "error", "{e}" }
                button { disabled: (ed.busy)(), onclick: move |_| reload(), "Retry" }
            } else {
                p { class: "placeholder", "Reading sensor settings…" }
            }
        };
    };
    let busy = (ed.busy)();
    let pct = |r: &std::ops::RangeInclusive<u32>| (*r.start() as f64 / 10.0, *r.end() as f64 / 10.0);
    let (pointer_min, pointer_max) = pct(&sensor::POINTER_RANGE);
    let (twist_min, twist_max) = pct(&sensor::TWIST_RANGE);

    rsx! {
        section { class: "card",
            h2 { "Pointer" }
            Slider {
                label: "Sensitivity",
                disabled: busy,
                value: f.pointer as f64 / 10.0,
                min: pointer_min,
                max: pointer_max,
                step: 0.1,
                unit: "%",
                on_change: move |v: f64| ed.edit(|s| s.pointer = (v * 10.0).round() as u32),
            }
            if let Some(deg) = f.plane_deg {
                Slider {
                    label: "Rotation",
                    disabled: busy,
                    value: deg as f64,
                    min: *sensor::PLANE_DEG_RANGE.start() as f64,
                    max: *sensor::PLANE_DEG_RANGE.end() as f64,
                    step: 1.0,
                    unit: "°",
                    on_change: move |v: f64| ed.edit(|s| s.plane_deg = Some(v.round() as i32)),
                }
            }
        }
        section { class: "card",
            h2 { "Twist scroll" }
            Toggle {
                label: "Enabled",
                disabled: busy,
                checked: f.twist_on,
                on_change: move |v: bool| ed.edit(|s| s.twist_on = v),
            }
            Slider {
                label: "Sensitivity",
                disabled: busy,
                value: f.twist as f64 / 10.0,
                min: twist_min,
                max: twist_max,
                step: 0.1,
                unit: "%",
                on_change: move |v: f64| ed.edit(|s| s.twist = (v * 10.0).round() as u32),
            }
            Toggle {
                label: "Reversed",
                disabled: busy,
                checked: f.twist_reversed,
                on_change: move |v: bool| ed.edit(|s| s.twist_reversed = v),
            }
        }
        section { class: "card",
            h2 { "Smoothing" }
            Toggle {
                label: "SMA smoothing",
                disabled: busy,
                checked: f.sma_on,
                on_change: move |v: bool| ed.edit(|s| s.sma_on = v),
            }
            Slider {
                label: "Window",
                disabled: busy,
                value: f.sma_window as f64,
                min: *sensor::SMA_WINDOW_RANGE.start() as f64,
                max: *sensor::SMA_WINDOW_RANGE.end() as f64,
                step: 1.0,
                unit: " samples",
                on_change: move |v: f64| ed.edit(|s| s.sma_window = v.round() as u32),
            }
            Slider {
                label: "Sync window",
                disabled: busy,
                value: f.sync_ms as f64,
                min: *sensor::SYNC_MS_RANGE.start() as f64,
                max: *sensor::SYNC_MS_RANGE.end() as f64,
                step: 1.0,
                unit: " ms",
                on_change: move |v: f64| ed.edit(|s| s.sync_ms = v.round() as u32),
            }
        }
        Actions {
            error: ed.error.cloned(),
            busy,
            dirty: ed.dirty(),
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
