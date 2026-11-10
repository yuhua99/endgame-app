//! Form rows and the Apply/Revert/Reload footer.

use dioxus::prelude::*;

#[component]
pub fn Slider(
    #[props(into)] label: String,
    disabled: bool,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    #[props(into)] unit: String,
    on_change: EventHandler<f64>,
) -> Element {
    let shown = if step < 1.0 { format!("{value:.1}") } else { format!("{value:.0}") };
    rsx! {
        label { class: "row slider",
            span { class: "label", "{label}" }
            input {
                r#type: "range",
                disabled,
                min: "{min}",
                max: "{max}",
                step: "{step}",
                value: "{value}",
                oninput: move |e| {
                    if let Ok(v) = e.value().parse::<f64>() {
                        on_change.call(v);
                    }
                },
            }
            span { class: "value", "{shown}{unit}" }
        }
    }
}

#[component]
pub fn Toggle(#[props(into)] label: String, disabled: bool, checked: bool, on_change: EventHandler<bool>) -> Element {
    rsx! {
        label { class: "row",
            span { class: "label", "{label}" }
            input { r#type: "checkbox", disabled, checked, onchange: move |e| on_change.call(e.checked()) }
        }
    }
}

/// Integer field; out-of-range input is clamped.
#[component]
pub fn Number(
    #[props(into)] label: String,
    disabled: bool,
    value: i32,
    min: i32,
    max: i32,
    #[props(into)] hint: String,
    on_change: EventHandler<i32>,
) -> Element {
    rsx! {
        label { class: "row",
            span { class: "label", "{label}" }
            input {
                r#type: "number",
                disabled,
                min: "{min}",
                max: "{max}",
                value: "{value}",
                // Parse as f64: number inputs also accept forms like "1e2".
                onchange: move |e| {
                    if let Ok(v) = e.value().trim().parse::<f64>() {
                        if v.is_finite() {
                            on_change.call(v.round().clamp(min as f64, max as f64) as i32);
                        }
                    }
                },
            }
            span { class: "value muted", "{hint}" }
        }
    }
}

/// The one place for a tab's error message and action buttons: a bar at the bottom of the tab.
/// Render it as the tab's last element.
#[component]
pub fn ActionBar(error: Option<String>, children: Element) -> Element {
    rsx! {
        footer { class: "actions",
            if let Some(e) = error.as_ref() {
                span { class: "error", "{e}" }
            }
            {children}
        }
    }
}

#[component]
pub fn Actions(
    error: Option<String>,
    busy: bool,
    dirty: bool,
    on_revert: EventHandler,
    on_reload: EventHandler,
    on_apply: EventHandler,
) -> Element {
    rsx! {
        ActionBar { error,
            button { class: "ghost", disabled: busy || !dirty, onclick: move |_| on_revert.call(()), "Revert" }
            button { class: "ghost", disabled: busy, onclick: move |_| on_reload.call(()), "Reload" }
            button { disabled: busy || !dirty, onclick: move |_| on_apply.call(()),
                if busy { "Working…" } else { "Apply" }
            }
        }
    }
}
