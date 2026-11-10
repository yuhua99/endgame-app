use std::time::Duration;

use dioxus::prelude::*;
use eg_device::sensor::{self, SensorSettings, Surface};
use eg_device::Session;

use crate::editable::use_editable;
use crate::widgets::{Actions, Slider, Toggle};

/// Pause between surface quality readings.
const SURFACE_POLL: Duration = Duration::from_millis(100);

/// `part / whole`, or 0 when there is no whole.
fn share(part: u32, whole: u32) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

/// Bar colour class. The colour changes at the same readings as in Marshmellow UI,
/// which rates a reported maximum of 728 on a 0-1000 scale.
fn level(reading: &Surface) -> &'static str {
    let scale = if reading.max == 728 { 1000 } else { reading.max };
    let (mid, high) = match scale {
        361 => (0.30, 0.60),
        1000 => (0.25, 0.50),
        _ => (0.34, 0.67),
    };
    match share(reading.quality, scale) {
        r if r < mid => "low",
        r if r < high => "mid",
        _ => "high",
    }
}

#[component]
pub fn SensorTab(session: Signal<Option<Session>>, visible: ReadSignal<bool>) -> Element {
    let ed = use_editable::<SensorSettings>();
    let shell = move || session.peek().as_ref().map(|s| s.shell().clone());
    let reload = move || async move {
        if let Some(shell) = shell() {
            ed.run(async move { sensor::read(&shell).await }).await;
        }
    };
    let apply = move || async move {
        if let (Some(shell), Some(saved), Some(new)) = (shell(), ed.saved.cloned(), ed.form.cloned()) {
            ed.run(async move { sensor::apply(&shell, &saved, &new).await }).await;
        }
    };
    // Other tabs can change these values on the device, so read them again whenever this tab
    // opens, unless there are edits to keep.
    use_effect(move || {
        if visible() && ed.idle() {
            spawn(reload());
        }
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
        SurfaceCard { session, visible }
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
            if let Some(on) = f.frame_sync {
                Toggle {
                    label: "Frame sync",
                    disabled: busy,
                    checked: on,
                    on_change: move |v: bool| ed.edit(|s| s.frame_sync = Some(v)),
                }
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

/// Live surface quality of each sensor, read repeatedly while the tab is visible.
#[component]
fn SurfaceCard(session: Signal<Option<Session>>, visible: ReadSignal<bool>) -> Element {
    let mut state = use_signal(|| Ok::<Vec<Surface>, String>(Vec::new()));
    use_hook(move || {
        spawn(async move {
            loop {
                if *visible.peek() {
                    let Some(shell) = session.peek().as_ref().map(|s| s.shell().clone()) else {
                        break;
                    };
                    let (new, done) = match sensor::surface(&shell).await {
                        Ok(readings) => (Ok(readings), false),
                        Err(e) if e.is_unsupported() => (Err("Not available on this firmware.".to_string()), true),
                        Err(e) => (Err(e.to_string()), false),
                    };
                    if *state.peek() != new {
                        state.set(new);
                    }
                    if done {
                        break;
                    }
                }
                tokio::time::sleep(SURFACE_POLL).await;
            }
        });
    });

    rsx! {
        section { class: "card",
            h2 { "Surface quality" }
            {
                match state() {
                    Ok(readings) if readings.is_empty() => rsx! { p { class: "placeholder", "Reading…" } },
                    Ok(readings) => rsx! {
                        for r in readings {
                            SurfaceRow { key: "{r.sensor}", reading: r }
                        }
                    },
                    Err(e) => rsx! { p { class: "placeholder", "{e}" } },
                }
            }
        }
    }
}

#[component]
fn SurfaceRow(reading: Surface) -> Element {
    let pct = (share(reading.quality, reading.max) * 100.0).min(100.0);
    rsx! {
        div { class: "row",
            span { class: "label", "Sensor #{reading.sensor}" }
            div { class: "meter",
                div { class: level(&reading), width: "{pct}%" }
            }
            span { class: "value", "{reading.quality}/{reading.max}" }
        }
    }
}
