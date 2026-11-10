//! The lights and the vibration the trackball gives on events: a list of the events beside
//! the settings of the selected one. The first row holds the light switch and the brightness.

use dioxus::prelude::*;
use eg_device::lighting::{self, Anim, Event, Group, Lighting, Rgb};
use eg_device::Session;

use crate::editable::{use_editable, Editable};
use crate::widgets::{Actions, Number, Slider, Toggle};

#[component]
pub fn LightingTab(session: Signal<Option<Session>>, visible: ReadSignal<bool>) -> Element {
    let ed = use_editable::<Lighting>();
    // Index of the event shown in the editor; `None` shows the general settings.
    let mut selected = use_signal(|| None::<usize>);
    let shell = move || session.peek().as_ref().map(|s| s.shell().clone());
    let reload = move || async move {
        if let Some(shell) = shell() {
            ed.run(async move { lighting::read(&shell).await }).await;
        }
    };
    let apply = move || async move {
        if let (Some(shell), Some(saved), Some(new)) = (shell(), ed.saved.cloned(), ed.form.cloned()) {
            ed.run(async move { lighting::apply(&shell, &saved, &new).await }).await;
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
                p { class: "placeholder", "Reading lighting settings…" }
            }
        };
    };
    let busy = (ed.busy)();
    // With the lights off only the vibration is left to set, so everything about light is hidden.
    let lights = f.on;
    let general = match (lights, f.brightness) {
        (false, _) => "Lights off".to_string(),
        (true, Some(pct)) => format!("On · {pct}%"),
        (true, None) => "On".to_string(),
    };
    let groups = [(Group::Layers, "Layers"), (Group::Alarms, "Alarms"), (Group::Events, "Events")];

    rsx! {
        if !f.rgb_supported {
            RgbOverride { session }
        }
        div { class: if lights { "lighting" } else { "lighting dark" },
            div {
                div { class: "event head",
                    if lights {
                        span {}
                    }
                    span { "Event" }
                    if lights {
                        span { "Light" }
                    }
                    span { "Vibration" }
                }
                div {
                    class: if selected().is_none() { "event selected" } else { "event" },
                    onclick: move |_| selected.set(None),
                    if lights {
                        span {}
                    }
                    span { "General" }
                    span { class: "muted", "{general}" }
                }
                for (group, title) in groups {
                    h2 { class: "group", "{title}" }
                    for (i, e) in f.events.iter().enumerate().filter(|(_, e)| e.group() == group) {
                        div {
                            key: "{e.name}",
                            class: if selected() == Some(i) { "event selected" } else { "event" },
                            onclick: move |_| selected.set(Some(i)),
                            if lights {
                                span { class: "swatch", background: e.colors.first().map_or(String::new(), |c| hex(*c)) }
                            }
                            span { "{e.title(&f.layer_names)}" }
                            if lights {
                                span { class: "muted", "{light_text(e)}" }
                            }
                            span { class: "muted", "{vibration_text(&e.pattern)}" }
                        }
                    }
                }
            }
            div {
                {
                    match selected().and_then(|i| Some((i, f.events.get(i)?))) {
                        Some((i, e)) => event_editor(ed, i, e, &f.layer_names, lights, busy),
                        None => rsx! {
                            section { class: "card form",
                                h2 { "General" }
                                Toggle {
                                    label: "Lights",
                                    disabled: busy,
                                    checked: lights,
                                    on_change: move |v: bool| ed.edit(|l| l.on = v),
                                }
                                if let Some(pct) = f.brightness.filter(|_| lights) {
                                    Slider {
                                        label: "Brightness",
                                        disabled: busy,
                                        value: pct as f64,
                                        min: *lighting::BRIGHTNESS_RANGE.start() as f64,
                                        max: *lighting::BRIGHTNESS_RANGE.end() as f64,
                                        step: 1.0,
                                        unit: "%",
                                        on_change: move |v: f64| ed.edit(|l| l.brightness = Some(v.round() as u32)),
                                    }
                                }
                            }
                        },
                    }
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

/// Shown while the board says it has no RGB lights. Forcing them on cannot be taken back,
/// so it asks first.
#[component]
fn RgbOverride(session: Signal<Option<Session>>) -> Element {
    let mut asking = use_signal(|| false);
    let mut result = use_signal(|| None::<Result<(), String>>);
    let force = move |_| async move {
        asking.set(false);
        if let Some(shell) = session.peek().as_ref().map(|s| s.shell().clone()) {
            result.set(Some(lighting::override_rgb(&shell).await.map_err(|e| e.to_string())));
        }
    };

    rsx! {
        div { class: "card banner notice",
            if result() == Some(Ok(())) {
                span { "RGB lights are now forced on. Restart the trackball for this to take effect." }
            } else if asking() {
                span { "Forcing the lights on is stored on the trackball and cannot be undone from this app. Continue?" }
                button { onclick: force, "Force on" }
                button { class: "ghost", onclick: move |_| asking.set(false), "Cancel" }
            } else {
                span {
                    "This trackball reports no RGB lights: colors and animations have no effect, vibration still works."
                    if let Some(Err(e)) = result() {
                        span { class: "error", " {e}" }
                    }
                }
                button { class: "ghost", onclick: move |_| asking.set(true), "Force RGB lights on…" }
            }
        }
    }
}

/// Settings of the event at index `i`.
fn event_editor(
    ed: Editable<Lighting>,
    i: usize,
    e: &Event,
    layer_names: &[String],
    lights: bool,
    busy: bool,
) -> Element {
    let title = e.title(layer_names);
    let pattern = pattern_text(&e.pattern);
    let pattern_max = lighting::PATTERN_MAX;
    rsx! {
        section { class: "card form",
            h2 { "{title}" }
            if lights {
                {light_rows(ed, i, e, busy)}
            }
            label { class: "row",
                span { class: "label", "Vibration" }
                input {
                    r#type: "text",
                    disabled: busy,
                    title: "Vibration and pause lengths in turn, starting with a vibration; at most {pattern_max} numbers",
                    value: "{pattern}",
                    onchange: move |ev| {
                        let values = parse_pattern(&ev.value());
                        ed.edit(|l| l.events[i].pattern = values);
                    },
                }
                span { class: "value muted", "ms" }
            }
        }
    }
}

/// Animation, colors, and the timings of the animation in use.
fn light_rows(ed: Editable<Lighting>, i: usize, e: &Event, busy: bool) -> Element {
    let (ms_min, ms_max) = (lighting::MS_MIN as i32, lighting::MS_MAX as i32);
    let (flash, fade_in, fade_out) = (e.flash_ms as i32, e.ease_in.ms as i32, e.ease_out.ms as i32);
    let cycles = matches!(e.anim, Anim::Blink | Anim::Breathe);
    rsx! {
        if e.is_layer() {
            label { class: "row",
                span { class: "label", "Animation" }
                select {
                    disabled: busy,
                    onchange: move |ev| {
                        if let Some(anim) = Anim::ALL.into_iter().find(|a| a.name() == ev.value()) {
                            ed.edit(|l| l.events[i].set_anim(anim));
                        }
                    },
                    for a in Anim::ALL {
                        option { value: a.name(), selected: a == e.anim, "{anim_label(a)}" }
                    }
                }
            }
        } else {
            div { class: "row",
                span { class: "label", "Animation" }
                span { "{anim_label(e.anim)}" }
            }
        }
        div { class: "row",
            span { class: "label", "Colors" }
            span { class: "swatches",
                for (c, color) in e.colors.iter().enumerate() {
                    input {
                        r#type: "color",
                        disabled: busy,
                        value: hex(*color),
                        onchange: move |ev| {
                            if let Some(rgb) = parse_hex(&ev.value()) {
                                ed.edit(|l| l.events[i].colors[c] = rgb);
                            }
                        },
                    }
                }
                // Only an animation that goes through its colors has use for more than one.
                if e.colors.is_empty() || cycles && e.colors.len() < lighting::COLORS_MAX {
                    button {
                        class: "ghost small",
                        disabled: busy,
                        title: "Add a color",
                        onclick: move |_| ed.edit(|l| l.events[i].colors.push(Rgb { r: 128, g: 128, b: 128 })),
                        "+"
                    }
                }
                if e.colors.len() > 1 {
                    button {
                        class: "ghost small",
                        disabled: busy,
                        title: "Remove the last color",
                        onclick: move |_| ed.edit(|l| {
                            l.events[i].colors.pop();
                        }),
                        "−"
                    }
                }
            }
        }
        {
            match e.anim {
                Anim::Solid => rsx! {},
                Anim::Blink => rsx! {
                    Number {
                        label: "On",
                        disabled: busy,
                        value: e.blink_on_ms as i32,
                        min: ms_min,
                        max: ms_max,
                        hint: "ms",
                        on_change: move |v: i32| ed.edit(|l| l.events[i].blink_on_ms = v as u32),
                    }
                    Number {
                        label: "Off",
                        disabled: busy,
                        value: e.blink_off_ms as i32,
                        min: ms_min,
                        max: ms_max,
                        hint: "ms",
                        on_change: move |v: i32| ed.edit(|l| l.events[i].blink_off_ms = v as u32),
                    }
                },
                Anim::Breathe => rsx! {
                    Number {
                        label: "Period",
                        disabled: busy,
                        value: e.breathe_ms as i32,
                        min: 0,
                        max: ms_max,
                        hint: "ms",
                        on_change: move |v: i32| ed.edit(|l| l.events[i].breathe_ms = v as u32),
                    }
                },
                // A flash has to outlast its fades; the bounds below keep it so.
                Anim::Flash => rsx! {
                    Number {
                        label: "Length",
                        disabled: busy,
                        value: flash,
                        min: (fade_in + fade_out + 1).clamp(ms_min, ms_max),
                        max: ms_max,
                        hint: "ms",
                        on_change: move |v: i32| ed.edit(|l| l.events[i].flash_ms = v as u32),
                    }
                    {
                        fade_row(
                            "Fade in",
                            busy,
                            fade_in,
                            (flash - fade_out - 1).max(0),
                            &e.ease_in.function,
                            move |ms| ed.edit(|l| l.events[i].ease_in.ms = ms),
                            move |function| ed.edit(|l| l.events[i].ease_in.function = function),
                        )
                    }
                    {
                        fade_row(
                            "Fade out",
                            busy,
                            fade_out,
                            (flash - fade_in - 1).max(0),
                            &e.ease_out.function,
                            move |ms| ed.edit(|l| l.events[i].ease_out.ms = ms),
                            move |function| ed.edit(|l| l.events[i].ease_out.function = function),
                        )
                    }
                },
            }
        }
    }
}

/// A fade's length, from 0 to `max` ms, and its easing function on one row.
fn fade_row(
    label: &'static str,
    disabled: bool,
    ms: i32,
    max: i32,
    function: &str,
    mut on_ms: impl FnMut(u32) + 'static,
    mut on_function: impl FnMut(String) + 'static,
) -> Element {
    rsx! {
        div { class: "row fade",
            span { class: "label", "{label}" }
            input {
                r#type: "number",
                disabled,
                min: "0",
                max: "{max}",
                value: "{ms}",
                // As in `Number`: what was typed is clamped.
                onchange: move |ev| {
                    if let Ok(v) = ev.value().trim().parse::<f64>() {
                        if v.is_finite() {
                            on_ms(v.round().clamp(0.0, max as f64) as u32);
                        }
                    }
                },
            }
            span { class: "muted", "ms" }
            select { disabled, onchange: move |ev| on_function(ev.value()),
                for name in lighting::EASINGS {
                    option { value: name, selected: name == function, "{name}" }
                }
            }
        }
    }
}

fn anim_label(anim: Anim) -> &'static str {
    match anim {
        Anim::Solid => "Solid",
        Anim::Blink => "Blink",
        Anim::Breathe => "Breathe",
        Anim::Flash => "Flash",
    }
}

/// "Solid", "Blink 750/750 ms", "Flash 350 ms".
fn light_text(e: &Event) -> String {
    let timing = match e.anim {
        Anim::Solid => String::new(),
        Anim::Blink => format!(" {}/{} ms", e.blink_on_ms, e.blink_off_ms),
        Anim::Breathe => format!(" {}", duration(e.breathe_ms)),
        Anim::Flash => format!(" {}", duration(e.flash_ms)),
    };
    format!("{}{timing}", anim_label(e.anim))
}

/// "350 ms", "1.5 s", "30 s".
fn duration(ms: u32) -> String {
    match ms {
        0..1000 => format!("{ms} ms"),
        _ if ms.is_multiple_of(1000) => format!("{} s", ms / 1000),
        _ => format!("{:.1} s", ms as f64 / 1000.0),
    }
}

/// "65, 25, 65 ms", or a dash where nothing vibrates.
fn vibration_text(pattern: &[u32]) -> String {
    if pattern.iter().all(|ms| *ms == 0) {
        "—".to_string()
    } else {
        format!("{} ms", pattern_text(pattern))
    }
}

/// "65, 25, 65"
fn pattern_text(pattern: &[u32]) -> String {
    pattern.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
}

/// "#8000ff", as a color input takes and gives it.
fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

fn parse_hex(value: &str) -> Option<Rgb> {
    let n = u32::from_str_radix(value.strip_prefix('#')?, 16).ok()?;
    Some(Rgb { r: (n >> 16) as u8, g: (n >> 8) as u8, b: n as u8 })
}

/// The numbers in `text`, whatever separates them, within the firmware's limits.
fn parse_pattern(text: &str) -> Vec<u32> {
    let numbers = text.split(|c: char| !c.is_ascii_digit()).filter_map(|v| v.parse::<u32>().ok());
    numbers.map(|ms| ms.min(lighting::MS_MAX)).take(lighting::PATTERN_MAX).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_round_trip_as_hex() {
        let c = Rgb { r: 128, g: 0, b: 255 };
        assert_eq!(hex(c), "#8000ff");
        assert_eq!(parse_hex("#8000ff"), Some(c));
        assert_eq!(parse_hex("8000ff"), None);
    }

    #[test]
    fn pattern_keeps_the_first_numbers() {
        assert_eq!(parse_pattern("65, 25 70000"), [65, 25, lighting::MS_MAX]);
        assert_eq!(parse_pattern("1 2 3 4 5 6 7 8 9 10 11").len(), lighting::PATTERN_MAX);
    }

    #[test]
    fn list_texts() {
        assert_eq!((duration(350), duration(1500), duration(30000)), ("350 ms".into(), "1.5 s".into(), "30 s".into()));
        assert_eq!(vibration_text(&[65, 25, 65]), "65, 25, 65 ms");
        assert_eq!(vibration_text(&[]), "—");
        assert_eq!(vibration_text(&[0]), "—");
    }
}
