use std::time::Duration;

use dioxus::prelude::*;
use eg_device::sensor::{self, Behavior, SensorSettings, Surface};
use eg_device::Session;

use crate::editable::{use_editable, Editable};
use crate::widgets::{Actions, Number, Slider, Toggle};

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
        div { class: "cards",
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
        }
        if !f.behaviors.is_empty() {
            p { class: "placeholder",
                "The behaviors below change a sensitivity when their key is pressed or their encoder is turned. Assign them in the Keymap tab."
            }
            div { class: "cards",
                for (i, b) in f.behaviors.iter().enumerate() {
                    {behavior_card(ed, i, b, busy)}
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

/// Settings of the behavior at index `i`.
fn behavior_card(ed: Editable<SensorSettings>, i: usize, b: &Behavior, busy: bool) -> Element {
    let (step_min, step_max) = (*sensor::STEP_RANGE.start() as i32, *sensor::STEP_RANGE.end() as i32);
    // As the firmware works it out: the highest sensitivity stops at the multiplier, and at
    // 100 % for the pointer; the lowest has no such cap.
    let cap = if b.scroll { b.max_multiplier * 1000 } else { 1000.min(b.max_multiplier * 1000) };
    let pct = |units: u32| format!("{:.1}%", units as f64 / 10.0);
    let above_lowest = b.min_step as i32 + 1;
    let pattern = b.pattern.iter().map(i32::to_string).collect::<Vec<_>>().join(", ");
    let pattern_max = sensor::PATTERN_MAX;
    rsx! {
        section { class: "card",
            h2 { "{b.name}" }
            Number {
                label: "Step (0.1 %)",
                disabled: busy,
                value: b.step as i32,
                min: step_min,
                max: step_max,
                hint: pct(b.step),
                on_change: move |v: i32| ed.edit(|s| s.behaviors[i].step = v as u32),
            }
            // The firmware needs lowest < highest; the bounds below keep it so.
            Number {
                label: "Lowest (steps)",
                disabled: busy,
                value: b.min_step as i32,
                min: step_min,
                max: (b.max_step as i32 - 1).max(step_min),
                hint: pct(b.min_step * b.step),
                on_change: move |v: i32| ed.edit(|s| s.behaviors[i].min_step = v as u32),
            }
            Number {
                label: "Highest (steps)",
                disabled: busy,
                value: b.max_step as i32,
                min: above_lowest,
                max: step_max.max(above_lowest),
                hint: pct((b.max_step * b.step).min(cap)),
                on_change: move |v: i32| ed.edit(|s| s.behaviors[i].max_step = v as u32),
            }
            if b.scroll {
                Slider {
                    label: "Highest multiplier",
                    disabled: busy,
                    value: b.max_multiplier as f64,
                    min: *sensor::MULTIPLIER_RANGE.start() as f64,
                    max: *sensor::MULTIPLIER_RANGE.end() as f64,
                    step: 1.0,
                    unit: "×",
                    on_change: move |v: f64| ed.edit(|s| s.behaviors[i].max_multiplier = v.round() as u32),
                }
            }
            Number {
                label: "Vibration",
                disabled: busy,
                value: b.feedback_duration as i32,
                min: 0,
                max: sensor::FEEDBACK_MS_MAX as i32,
                hint: "ms",
                on_change: move |v: i32| ed.edit(|s| s.behaviors[i].feedback_duration = v as u32),
            }
            Toggle {
                label: "Wrap around",
                disabled: busy,
                checked: b.wrap,
                on_change: move |v: bool| ed.edit(|s| s.behaviors[i].wrap = v),
            }
            if !b.wrap {
                Toggle {
                    label: "Feedback on limit",
                    disabled: busy,
                    checked: b.feedback_on_limit,
                    on_change: move |v: bool| ed.edit(|s| s.behaviors[i].feedback_on_limit = v),
                }
            }
            if b.wrap || b.feedback_on_limit {
                label { class: "row",
                    span { class: "label", "Pattern" }
                    input {
                        r#type: "text",
                        disabled: busy,
                        title: "Vibration and pause lengths in turn, starting with a vibration; at most {pattern_max} numbers",
                        value: "{pattern}",
                        onchange: move |e| {
                            let values = parse_pattern(&e.value());
                            ed.edit(|s| s.behaviors[i].pattern = values);
                        },
                    }
                    span { class: "value muted", "ms" }
                }
            }
        }
    }
}

/// The numbers in `text`, whatever separates them, up to the firmware's limit.
fn parse_pattern(text: &str) -> Vec<i32> {
    text.split(|c: char| !c.is_ascii_digit()).filter_map(|v| v.parse().ok()).take(sensor::PATTERN_MAX).collect()
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
