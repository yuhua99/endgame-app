use dioxus::prelude::*;
use eg_device::keymap::{
    self as km, Behavior, BehaviorBinding, Event, KeyPhysicalAttrs, Keymap, Layer, ParamDesc, PhysicalLayout, ValueType,
};
use eg_device::Session;

use crate::endgame::{self, EndgameLayout};
use crate::hid;

/// Pixels per key unit; layout values are in 1/100 key units.
const UNIT: f64 = 48.0;

#[derive(Clone, PartialEq)]
struct Data {
    layout: PhysicalLayout,
    keymap: Keymap,
    behaviors: Vec<Behavior>,
}

/// Studio-based key remapping. Changes apply to the device immediately; Save persists them.
#[component]
pub fn KeymapTab(session: Signal<Option<Session>>) -> Element {
    let mut locked = use_signal(|| None::<bool>);
    let mut data = use_signal(|| None::<Data>);
    let mut unsaved = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut layer_idx = use_signal(|| 0usize);
    let mut selected = use_signal(|| None::<usize>);
    // Loads are coalesced: a request during a load makes that load run once more.
    let mut loading = use_signal(|| false);
    let mut load_again = use_signal(|| false);

    let studio = move || session.peek().as_ref().and_then(|s| s.studio().cloned());
    let mut fail = move |e: eg_device::Error| {
        if e.is_unlock_required() {
            locked.set(Some(true));
        }
        error.set(Some(e.to_string()));
    };
    let load = move || async move {
        let Some(st) = studio() else { return };
        if *loading.peek() {
            load_again.set(true);
            return;
        }
        loading.set(true);
        busy.set(true);
        loop {
            load_again.set(false);
            let result = async {
                if km::is_locked(&st).await? {
                    return Ok(None);
                }
                let d = Data {
                    layout: km::layout(&st).await?,
                    keymap: km::keymap(&st).await?,
                    behaviors: km::behaviors(&st).await?,
                };
                Ok::<_, eg_device::Error>(Some((d, km::has_unsaved(&st).await?)))
            }
            .await;
            match result {
                Ok(Some((d, u))) => {
                    locked.set(Some(false));
                    data.set(Some(d));
                    unsaved.set(u);
                    error.set(None);
                }
                Ok(None) => locked.set(Some(true)),
                Err(e) => fail(e),
            }
            if !*load_again.peek() {
                break;
            }
        }
        busy.set(false);
        loading.set(false);
    };
    // Runs a change, then refreshes the keymap and unsaved status from the device.
    let change = move |op: ChangeOp| async move {
        let Some(st) = studio() else { return };
        busy.set(true);
        let result = async {
            match op {
                ChangeOp::Bind {
                    layer_id,
                    position,
                    binding,
                } => km::set_binding(&st, layer_id, position, binding).await?,
                ChangeOp::Save => km::save(&st).await?,
                ChangeOp::Discard => km::discard(&st).await?,
            }
            Ok::<_, eg_device::Error>((km::keymap(&st).await?, km::has_unsaved(&st).await?))
        }
        .await;
        match result {
            Ok((k, u)) => {
                if let Some(d) = data.write().as_mut() {
                    d.keymap = k;
                }
                unsaved.set(u);
                error.set(None);
            }
            Err(e) => fail(e),
        }
        busy.set(false);
    };
    use_hook(move || {
        let Some(st) = studio() else { return };
        let mut events = km::events(&st);
        spawn(async move {
            while let Some(ev) = events.next().await {
                match ev {
                    Event::Locked(l) => {
                        locked.set(Some(l));
                        // Even if a load is running: its lock check may predate the unlock,
                        // so `load` then runs once more.
                        if !l && data.peek().is_none() {
                            spawn(load());
                        }
                    }
                    Event::Unsaved(u) => unsaved.set(u),
                }
            }
        });
        spawn(load());
    });

    if studio().is_none() {
        return rsx! {
            p { class: "placeholder", "Keymap editing needs ZMK Studio, which this connection does not provide." }
        };
    }
    let busy_now = busy();
    let lock_banner = rsx! {
        if locked() == Some(true) {
            div { class: "card banner",
                "The trackball is locked. Press its Studio Unlock key (on the Device layer); this tab continues automatically. "
                button { class: "ghost small", disabled: busy(), onclick: move |_| load(), "Check again" }
            }
        }
    };
    let Some(d) = data() else {
        return rsx! {
            {lock_banner}
            if let Some(e) = error() {
                p { class: "error", "{e}" }
                button { disabled: busy_now, onclick: move |_| load(), "Retry" }
            } else if locked() != Some(true) {
                p { class: "placeholder", "Loading keymap…" }
            }
        };
    };

    let layers = d.keymap.layers.clone();
    let li = layer_idx().min(layers.len().saturating_sub(1));
    let Some(layer) = layers.get(li).cloned() else {
        return rsx! { p { class: "placeholder", "The keymap has no layers." } };
    };
    let caps = key_caps(&d.layout.keys, &layer, &d.behaviors, &layers);
    let is_endgame = endgame::matches(&d.layout.keys);
    let (width, height) = layout_size(&d.layout.keys);
    let editing = selected().and_then(|pos| layer.bindings.get(pos).map(|b| (pos, *b)));

    rsx! {
        {lock_banner}
        div { class: "toolbar",
            for (i, l) in layers.iter().enumerate() {
                button {
                    key: "{l.id}",
                    class: if i == li { "tab active" } else { "tab" },
                    onclick: move |_| layer_idx.set(i),
                    if l.name.is_empty() { "{i}" } else { "{l.name}" }
                }
            }
            span { class: "spacer" }
            button {
                class: "ghost",
                disabled: busy_now || !unsaved(),
                onclick: move |_| change(ChangeOp::Discard),
                "Discard"
            }
            button { disabled: busy_now || !unsaved(), onclick: move |_| change(ChangeOp::Save), "Save" }
        }
        // The Endgame drawing stays on the left with the editor beside it; other layouts stack.
        div { class: if is_endgame { "keymap-body split" } else { "keymap-body" },
            div { class: "layout-wrap",
                if is_endgame {
                    EndgameLayout { caps, selected: selected(), on_select: move |pos| selected.set(Some(pos)) }
                } else {
                    div { class: "layout", style: "width: {width}px; height: {height}px;",
                        for cap in caps {
                            button {
                                key: "{cap.pos}",
                                class: if selected() == Some(cap.pos) { "key selected" } else { "key" },
                                style: "{cap.style}",
                                title: "{cap.title}",
                                onclick: move |_| selected.set(Some(cap.pos)),
                                span { class: "hdr", "{cap.header}" }
                                span { class: "lbl", "{cap.label}" }
                            }
                        }
                    }
                }
            }
            div {
                if let Some(e) = error() {
                    p { class: "error", "{e}" }
                }
                if let Some((pos, binding)) = editing {
                    BindingEditor {
                        key: "{layer.id}-{pos}",
                        position: pos,
                        binding,
                        behaviors: d.behaviors.clone(),
                        layers: layers.clone(),
                        disabled: busy_now || locked() == Some(true),
                        on_change: move |b: BehaviorBinding| {
                            spawn(change(ChangeOp::Bind { layer_id: layer.id, position: pos as i32, binding: b }));
                        },
                    }
                } else {
                    p { class: "placeholder", "Select a key to change its binding." }
                }
            }
        }
    }
}

enum ChangeOp {
    Bind {
        layer_id: u32,
        position: i32,
        binding: BehaviorBinding,
    },
    Save,
    Discard,
}

#[derive(Clone, PartialEq)]
pub(crate) struct Cap {
    pub pos: usize,
    style: String,
    pub header: String,
    pub label: String,
    title: String,
}

fn key_caps(keys: &[KeyPhysicalAttrs], layer: &Layer, behaviors: &[Behavior], layers: &[Layer]) -> Vec<Cap> {
    let u = |v: i32| v as f64 * UNIT / 100.0;
    keys.iter()
        .enumerate()
        .map(|(pos, k)| {
            let mut style = format!(
                "left: {}px; top: {}px; width: {}px; height: {}px;",
                u(k.x),
                u(k.y),
                u(k.width) - 2.0,
                u(k.height) - 2.0
            );
            if k.r != 0 {
                style += &format!(
                    " transform-origin: {}px {}px; transform: rotate({}deg);",
                    u(k.rx - k.x),
                    u(k.ry - k.y),
                    k.r as f64 / 100.0
                );
            }
            let (header, label, title) = match layer.bindings.get(pos) {
                Some(b) => binding_text(b, behaviors, layers),
                None => (String::new(), "?".into(), "No binding".into()),
            };
            Cap {
                pos,
                style,
                header,
                label,
                title,
            }
        })
        .collect()
}

fn layout_size(keys: &[KeyPhysicalAttrs]) -> (f64, f64) {
    let max = |f: fn(&KeyPhysicalAttrs) -> i32| keys.iter().map(f).max().unwrap_or(0) as f64 * UNIT / 100.0;
    (max(|k| k.x + k.width), max(|k| k.y + k.height))
}

/// (cap header, cap label, tooltip) for a binding.
fn binding_text(b: &BehaviorBinding, behaviors: &[Behavior], layers: &[Layer]) -> (String, String, String) {
    let Some(beh) = behaviors.iter().find(|x| x.id as i32 == b.behavior_id) else {
        return ("?".into(), String::new(), format!("Unknown behavior {}", b.behavior_id));
    };
    let ids: Vec<u32> = layers.iter().map(|l| l.id).collect();
    let (p1, p2) = km::param_descs(beh, b.param1, &ids);
    let label = [param_text(&p1, b.param1, layers), param_text(&p2, b.param2, layers)]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let title = if label.is_empty() {
        beh.display_name.clone()
    } else {
        format!("{}: {label}", beh.display_name)
    };
    (hid::behavior_short(&beh.display_name), label, title)
}

fn param_text(descs: &[ParamDesc], value: u32, layers: &[Layer]) -> String {
    let ids: Vec<u32> = layers.iter().map(|l| l.id).collect();
    let Some(d) = descs
        .iter()
        .find(|d| km::accepts(std::slice::from_ref(*d), value, &ids))
    else {
        return if descs.is_empty() {
            String::new()
        } else {
            value.to_string()
        };
    };
    match &d.value_type {
        Some(ValueType::HidUsage(_)) => hid::label(value),
        Some(ValueType::LayerId(_)) => layer_name(layers, value),
        Some(ValueType::Constant(_)) => d.name.clone(),
        Some(ValueType::Range(_)) => value.to_string(),
        Some(ValueType::Nil(_)) | None => String::new(),
    }
}

fn layer_name(layers: &[Layer], id: u32) -> String {
    layers.iter().enumerate().find(|(_, l)| l.id == id).map_or_else(
        || id.to_string(),
        |(i, l)| {
            if l.name.is_empty() {
                i.to_string()
            } else {
                l.name.clone()
            }
        },
    )
}

#[component]
fn BindingEditor(
    position: usize,
    binding: BehaviorBinding,
    behaviors: Vec<Behavior>,
    layers: Vec<Layer>,
    disabled: bool,
    on_change: EventHandler<BehaviorBinding>,
) -> Element {
    let ids: Vec<u32> = layers.iter().map(|l| l.id).collect();
    let current = behaviors.iter().find(|b| b.id as i32 == binding.behavior_id).cloned();
    let for_param1 = (current.clone(), ids.clone());
    let (p1, p2) = current
        .as_ref()
        .map(|b| km::param_descs(b, binding.param1, &ids))
        .unwrap_or_default();
    let options = behaviors.clone();
    rsx! {
        section { class: "card editor",
            h2 { "Key {position}" }
            label { class: "row",
                span { class: "label", "Behavior" }
                select {
                    disabled,
                    onchange: move |e| {
                        let picked = e.value().parse::<u32>().ok().and_then(|id| options.iter().find(|b| b.id == id));
                        if let Some(b) = picked {
                            on_change.call(km::default_binding(b, &ids));
                        }
                    },
                    if current.is_none() {
                        option { selected: true, "Unknown ({binding.behavior_id})" }
                    }
                    for b in behaviors.iter() {
                        option { key: "{b.id}", value: "{b.id}", selected: b.id as i32 == binding.behavior_id, "{b.display_name}" }
                    }
                }
            }
            if !p1.is_empty() {
                ParamPicker {
                    label: param_label(&p1, "Parameter 1"),
                    descs: p1.clone(),
                    value: binding.param1,
                    layers: layers.clone(),
                    disabled,
                    on_change: move |v: u32| {
                        if let (Some(b), ids) = &for_param1 {
                            on_change.call(km::with_param1(b, binding, v, ids));
                        }
                    },
                }
            }
            if !p2.is_empty() {
                ParamPicker {
                    label: param_label(&p2, "Parameter 2"),
                    descs: p2.clone(),
                    value: binding.param2,
                    layers: layers.clone(),
                    disabled,
                    on_change: move |v: u32| on_change.call(BehaviorBinding { param2: v, ..binding }),
                }
            }
        }
    }
}

/// The description name when all descriptions share it, else `fallback`.
fn param_label(descs: &[ParamDesc], fallback: &str) -> String {
    match descs.first() {
        Some(d) if !d.name.is_empty() && descs.iter().all(|x| x.name == d.name) => d.name.clone(),
        _ => fallback.to_string(),
    }
}

#[component]
fn ParamPicker(
    label: String,
    descs: Vec<ParamDesc>,
    value: u32,
    layers: Vec<Layer>,
    disabled: bool,
    on_change: EventHandler<u32>,
) -> Element {
    let all_constant = descs
        .iter()
        .all(|d| matches!(d.value_type, Some(ValueType::Constant(_))));
    let control = if all_constant {
        let known = descs.iter().any(|d| d.value_type == Some(ValueType::Constant(value)));
        rsx! {
            select {
                disabled,
                onchange: move |e| {
                    if let Ok(v) = e.value().parse::<u32>() {
                        on_change.call(v);
                    }
                },
                if !known {
                    option { selected: true, "{value}" }
                }
                for (c, name) in descs.iter().filter_map(|d| match &d.value_type {
                    Some(ValueType::Constant(c)) => Some((*c, d.name.clone())),
                    _ => None,
                }) {
                    option { key: "{c}", value: "{c}", selected: c == value, "{name}" }
                }
            }
        }
    } else {
        match descs.iter().find_map(|d| d.value_type) {
            Some(ValueType::Range(r)) => {
                let (lo, hi) = (r.min, r.max);
                rsx! {
                    input {
                        r#type: "number",
                        disabled,
                        min: "{lo}",
                        max: "{hi}",
                        value: "{value}",
                        onchange: move |e| {
                            if let Ok(v) = e.value().trim().parse::<f64>() {
                                if v.is_finite() {
                                    on_change.call(v.round().clamp(lo as f64, hi as f64) as i32 as u32);
                                }
                            }
                        },
                    }
                }
            }
            Some(ValueType::HidUsage(h)) => rsx! {
                HidPicker { value, keyboard_max: h.keyboard_max, consumer_max: h.consumer_max, disabled, on_change }
            },
            Some(ValueType::LayerId(_)) => rsx! {
                select {
                    disabled,
                    onchange: move |e| {
                        if let Ok(v) = e.value().parse::<u32>() {
                            on_change.call(v);
                        }
                    },
                    for (i, l) in layers.iter().enumerate() {
                        option { key: "{l.id}", value: "{l.id}", selected: l.id == value,
                            if l.name.is_empty() { "{i}" } else { "{l.name}" }
                        }
                    }
                }
            },
            _ => rsx! {},
        }
    };
    rsx! {
        label { class: "row",
            span { class: "label", "{label}" }
            {control}
        }
    }
}

#[component]
fn HidPicker(
    value: u32,
    keyboard_max: u32,
    consumer_max: u32,
    disabled: bool,
    on_change: EventHandler<u32>,
) -> Element {
    let mut filter = use_signal(String::new);
    let (mods, page, id) = hid::split(value);
    let usage = hid::join(0, page, id);
    let f = filter().to_lowercase();
    // Same limits as ZMK Studio: device-reported maxima per page; modifiers are always allowed.
    let allowed = |u: &hid::Usage| match u.page {
        7 => u.id <= keyboard_max || (0xE0..=0xE7).contains(&u.id),
        12 => u.id <= consumer_max,
        _ => false,
    };
    let options: Vec<&hid::Usage> = hid::usages()
        .iter()
        .filter(|u| {
            u.value() == usage
                || (allowed(u) && (f.is_empty() || u.long.to_lowercase().contains(&f) || u.short.to_lowercase() == f))
        })
        .collect();
    let known = options.iter().any(|u| u.value() == usage);
    rsx! {
        div { class: "hid",
            input {
                r#type: "search",
                placeholder: "Filter keys…",
                disabled,
                value: "{filter}",
                oninput: move |e| filter.set(e.value()),
            }
            select {
                disabled,
                onchange: move |e| {
                    if let Ok(v) = e.value().parse::<u32>() {
                        on_change.call(v | mods << 24);
                    }
                },
                if !known {
                    option { selected: true, "{hid::label(usage)}" }
                }
                for u in options {
                    option { key: "{u.value()}", value: "{u.value()}", selected: u.value() == usage,
                        if u.page == 12 { "Consumer · {u.long}" } else { "{u.long}" }
                    }
                }
            }
            div { class: "mods",
                for (bit, name, _) in hid::MODS {
                    label { key: "{name}",
                        input {
                            r#type: "checkbox",
                            disabled,
                            checked: mods & bit != 0,
                            onchange: move |e| {
                                let m = if e.checked() { mods | bit } else { mods & !bit };
                                on_change.call(usage | m << 24);
                            },
                        }
                        "{name}"
                    }
                }
            }
        }
    }
}
