//! Feedback on events: the light and vibration the trackball gives when, say, a layer turns
//! on or the battery runs low (`argb`), with the RGB brightness and the board's RGB support.
//!
//! `argb status`, `board rgb`, and `board layers` outputs are recorded from firmware v1.6.2
//! (tests/fixtures). The commands that change an event follow the firmware source and are
//! not yet tried on hardware.

use std::ops::RangeInclusive;

use eg_shell::Shell;

use crate::{exec_checked, exec_optional, fields, rtcfg, Error, Result};

const BRIGHTNESS: &str = "argb/brt";

pub const BRIGHTNESS_RANGE: RangeInclusive<u32> = 0..=100;
/// Shortest blink phase and flash, as in Marshmellow UI.
pub const MS_MIN: u32 = 10;
/// The firmware stores durations in 16 bits.
pub const MS_MAX: u32 = u16::MAX as u32;
/// Limits the Endgame firmware is built with.
pub const COLORS_MAX: usize = 4;
pub const PATTERN_MAX: usize = 9;
/// Easing functions, as the firmware names them.
pub const EASINGS: [&str; 14] = [
    "linear",
    "quad-in",
    "quad-out",
    "quad-in-out",
    "cubic-in",
    "cubic-out",
    "cubic-in-out",
    "quart-in",
    "quart-out",
    "quart-in-out",
    "expo-in",
    "expo-out",
    "bounce-out",
    "bounce-in",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Lighting {
    /// Whether the lights are on. Going by the firmware source, events vibrate either way.
    pub on: bool,
    /// RGB brightness in percent (an `rtcfg` parameter); `None` if the firmware lacks it.
    pub brightness: Option<u32>,
    pub events: Vec<Event>,
    /// False if the board found no RGB lights; colors and animations then do nothing.
    pub rgb_supported: bool,
    /// Layer names by layer number, for `Event::title`.
    pub layer_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// As the firmware names it: "layer 2", "batt-warn 1", "usb-conn".
    pub name: String,
    pub label: Option<String>,
    pub anim: Anim,
    /// Solid and flash use the first color; blink and breathe go through all of them.
    pub colors: Vec<Rgb>,
    pub blink_on_ms: u32,
    pub blink_off_ms: u32,
    /// Length of a flash, fades included.
    pub flash_ms: u32,
    pub ease_in: Ease,
    pub ease_out: Ease,
    pub breathe_ms: u32,
    /// Vibration: ms on, off, on, ...
    pub pattern: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Anim {
    Solid,
    Blink,
    Breathe,
    Flash,
}

impl Anim {
    pub const ALL: [Anim; 4] = [Anim::Solid, Anim::Blink, Anim::Breathe, Anim::Flash];

    /// The firmware's name.
    pub fn name(self) -> &'static str {
        match self {
            Anim::Solid => "solid",
            Anim::Blink => "blink",
            Anim::Breathe => "breathe",
            Anim::Flash => "flash",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// A fade at the start or end of a flash.
#[derive(Debug, Clone, PartialEq)]
pub struct Ease {
    pub ms: u32,
    /// One of `EASINGS`.
    pub function: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Group {
    Layers,
    Alarms,
    Events,
}

impl Event {
    /// The number at the end of names like "layer 2" and "batt-warn 1".
    fn number(&self) -> Option<usize> {
        self.name.rsplit_once(' ')?.1.parse().ok()
    }

    /// Only a layer's animation can be chosen; the others keep theirs.
    pub fn is_layer(&self) -> bool {
        self.name.starts_with("layer ")
    }

    pub fn group(&self) -> Group {
        match self.name.split(' ').next() {
            Some("layer") => Group::Layers,
            Some("batt-warn" | "batt-crit" | "no-endpoint" | "error") => Group::Alarms,
            _ => Group::Events,
        }
    }

    /// Name to show: the firmware's label, the layer's name, or else the event name.
    pub fn title(&self, layer_names: &[String]) -> String {
        match (&self.label, self.number()) {
            // The battery levels and the Bluetooth profiles each share one label.
            (Some(label), Some(n)) if self.name.starts_with("batt-") || self.name.starts_with("ble-profile") => {
                format!("{label} {n}")
            }
            (Some(label), _) => label.clone(),
            (None, Some(n)) if self.is_layer() => layer_names.get(n).cloned().unwrap_or_else(|| format!("Layer {n}")),
            (None, _) => self.name.clone(),
        }
    }

    /// Switches the animation and raises its timings to what it needs; the timings of an
    /// animation that was not in use are often 0.
    pub fn set_anim(&mut self, anim: Anim) {
        self.anim = anim;
        match anim {
            Anim::Blink => {
                self.blink_on_ms = self.blink_on_ms.max(MS_MIN);
                self.blink_off_ms = self.blink_off_ms.max(MS_MIN);
            }
            // A flash has to outlast its fades.
            Anim::Flash => {
                self.flash_ms = self.flash_ms.max(MS_MIN).max(self.ease_in.ms + self.ease_out.ms + 1).min(MS_MAX)
            }
            Anim::Solid | Anim::Breathe => {}
        }
    }
}

/// Fails with `Unsupported` on firmware without `argb` (see `Error::is_unsupported`).
pub async fn read(shell: &Shell) -> Result<Lighting> {
    let status = shell.exec("argb status").await?;
    let (on, events) =
        parse_status(&status).ok_or_else(|| Error::Parse { cmd: "argb status".into(), output: status.clone() })?;
    let rgb = exec_optional(shell, "board rgb").await?;
    let layers = exec_optional(shell, "board layers").await?;
    Ok(Lighting {
        on,
        brightness: rtcfg::get(shell, BRIGHTNESS).await?.and_then(|v| v.try_into().ok()),
        events,
        // Only an explicit "RGB support: no" counts.
        rgb_supported: rgb.is_none_or(|out| fields(&out, "RGB support:").next() != Some("no")),
        layer_names: layers.map(|out| parse_layers(&out)).unwrap_or_default(),
    })
}

/// Sends what differs between `saved` (what the edits started from) and `new`, then reads
/// back what the device has. The firmware writes `argb` changes to flash only after a delay.
pub async fn apply(shell: &Shell, saved: &Lighting, new: &Lighting) -> Result<Lighting> {
    for cmd in diff(saved, new) {
        if cmd.starts_with("argb ") {
            // These print nothing unless they fail ("error: -22", "unknown property 'x'").
            let out = shell.exec(&cmd).await?;
            if !out.trim().is_empty() {
                return Err(Error::Rejected(format!("{cmd}: {}", out.trim())));
            }
        } else {
            exec_checked(shell, &cmd).await?;
        }
    }
    read(shell).await
}

/// Makes the board treat its RGB lights as present, from the next restart on. The board
/// stores this and has no command to take it back.
pub async fn override_rgb(shell: &Shell) -> Result<()> {
    let out = shell.exec("board rgb override").await?;
    if out.contains("overridden") {
        Ok(())
    } else {
        Err(Error::Rejected(format!("board rgb override: {}", out.trim())))
    }
}

/// `argb status`: "state: on", then a "[name]" block per event. `None` if not understood.
fn parse_status(out: &str) -> Option<(bool, Vec<Event>)> {
    let on = match fields(out, "state:").next()? {
        "on" => true,
        "off" => false,
        _ => return None,
    };
    // Only block headers start a line with a bracket; color lines are indented.
    let events = out.split("\n[").skip(1).map(parse_event).collect::<Option<_>>()?;
    Some((on, events))
}

/// One block without its opening bracket: "layer 2]", then indented "key: value" lines.
fn parse_event(block: &str) -> Option<Event> {
    let (name, _) = block.split_once(']')?;
    let numbers_of = |key: &str| Some(numbers(fields(block, key).next()?));
    let ease = |key: &str| {
        let value = fields(block, key).next()?;
        // "250ms fn=linear"
        Some(Ease { ms: *numbers(value).first()?, function: value.split_once("fn=")?.1.trim().to_string() })
    };
    let anim = fields(block, "anim:").next()?;
    let blink = numbers_of("blink:")?;
    // "[0] r=128 g=0   b=255"
    let colors = block.lines().skip(1).filter(|line| line.trim_start().starts_with('['));
    Some(Event {
        name: name.to_string(),
        label: fields(block, "label:").next().map(str::to_string),
        anim: Anim::ALL.into_iter().find(|a| a.name() == anim)?,
        colors: colors
            .map(|line| match numbers(line)[..] {
                [_, r, g, b] => Some(Rgb { r: r.try_into().ok()?, g: g.try_into().ok()?, b: b.try_into().ok()? }),
                _ => None,
            })
            .collect::<Option<_>>()?,
        blink_on_ms: *blink.first()?,
        blink_off_ms: *blink.get(1)?,
        flash_ms: *numbers_of("flash-dur:")?.first()?,
        ease_in: ease("flash-ease-in:")?,
        ease_out: ease("flash-ease-out:")?,
        breathe_ms: *numbers_of("breathe:")?.first()?,
        // "[65 25 65]" or "(none)"
        pattern: numbers_of("feedback:")?,
    })
}

/// `board layers`: a "0: Default" line per layer.
fn parse_layers(out: &str) -> Vec<String> {
    let names = out.lines().filter_map(|line| {
        let (number, name) = line.split_once(':')?;
        number.trim().parse::<usize>().ok()?;
        Some(name.trim().to_string())
    });
    names.collect()
}

/// The unsigned numbers in `text`, in order.
fn numbers(text: &str) -> Vec<u32> {
    text.split(|c: char| !c.is_ascii_digit()).filter_map(|v| v.parse().ok()).collect()
}

/// Commands that turn `old` into `new`.
pub fn diff(old: &Lighting, new: &Lighting) -> Vec<String> {
    let mut cmds = Vec::new();
    if new.on != old.on {
        cmds.push(format!("argb {}", if new.on { "on" } else { "off" }));
    }
    if let Some(pct) = new.brightness.filter(|_| new.brightness != old.brightness) {
        cmds.push(format!("rtcfg set {BRIGHTNESS} {pct}"));
    }
    for e in &new.events {
        let Some(o) = old.events.iter().find(|o| o.name == e.name) else {
            continue;
        };
        let evt = format!("argb evt {}", e.name);
        if e.colors != o.colors {
            // Without an index the firmware sets the first color and drops the rest; with
            // one it sets that color and grows the list to hold it.
            for (i, c) in e.colors.iter().enumerate() {
                let index = if i == 0 { String::new() } else { format!(" {i}") };
                cmds.push(format!("{evt} color{index} {} {} {}", c.r, c.g, c.b));
            }
        }
        if e.anim != o.anim {
            cmds.push(format!("{evt} anim {}", e.anim.name()));
        }
        if (e.blink_on_ms, e.blink_off_ms) != (o.blink_on_ms, o.blink_off_ms) {
            cmds.push(format!("{evt} blink {} {}", e.blink_on_ms, e.blink_off_ms));
        }
        if e.flash_ms != o.flash_ms {
            cmds.push(format!("{evt} flash {}", e.flash_ms));
        }
        if e.ease_in != o.ease_in {
            cmds.push(format!("{evt} flash-ease-in {} {}", e.ease_in.ms, e.ease_in.function));
        }
        if e.ease_out != o.ease_out {
            cmds.push(format!("{evt} flash-ease-out {} {}", e.ease_out.ms, e.ease_out.function));
        }
        if e.breathe_ms != o.breathe_ms {
            cmds.push(format!("{evt} breathe {}", e.breathe_ms));
        }
        if e.pattern != o.pattern {
            // The command needs a value; a single 0 is as close to "none" as it gets.
            let values: String = e.pattern.iter().map(|v| format!(" {v}")).collect();
            cmds.push(format!("{evt} feedback{}", if values.is_empty() { " 0" } else { &values }));
        }
    }
    cmds
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = include_str!("../tests/fixtures/v1.6.2/argb_status.txt");

    fn sample() -> Lighting {
        let (on, events) = parse_status(STATUS).unwrap();
        let layer_names = parse_layers(include_str!("../tests/fixtures/v1.6.2/board_layers.txt"));
        Lighting { on, brightness: Some(50), events, rgb_supported: false, layer_names }
    }

    fn event(l: &Lighting, name: &str) -> usize {
        l.events.iter().position(|e| e.name == name).unwrap()
    }

    #[test]
    fn parses_recorded_status() {
        let l = sample();
        assert!(!l.on);
        assert_eq!(l.events.len(), 28);
        assert_eq!(
            l.events[0],
            Event {
                name: "error 0".into(),
                label: Some("Sensor error (left)".into()),
                anim: Anim::Blink,
                colors: vec![Rgb { r: 255, g: 0, b: 0 }],
                blink_on_ms: 750,
                blink_off_ms: 750,
                flash_ms: 0,
                ease_in: Ease { ms: 0, function: "linear".into() },
                ease_out: Ease { ms: 0, function: "linear".into() },
                breathe_ms: 0,
                pattern: vec![250],
            }
        );
        let layer = l.events.iter().find(|e| e.name == "layer 2").unwrap();
        assert_eq!(layer.label, None);
        assert_eq!((layer.anim, layer.flash_ms, layer.ease_in.ms), (Anim::Flash, 30000, 250));
        assert_eq!(layer.colors, [Rgb { r: 128, g: 0, b: 255 }]);
        assert!(l.events.iter().find(|e| e.name == "layer 0").unwrap().pattern.is_empty());
    }

    #[test]
    fn malformed_status_is_not_parsed() {
        assert_eq!(parse_status("argb: command not found"), None);
        assert_eq!(parse_status(&STATUS.replace("anim:       blink", "anim:       wobble")), None);
    }

    #[test]
    fn titles_and_groups() {
        let l = sample();
        let of = |name: &str| {
            let e = l.events.iter().find(|e| e.name == name).unwrap();
            (e.title(&l.layer_names), e.group())
        };
        assert_eq!(of("layer 2"), ("Device".into(), Group::Layers));
        assert_eq!(of("batt-warn 3"), ("Battery warning 3".into(), Group::Alarms));
        assert_eq!(of("error 1"), ("Sensor error (right)".into(), Group::Alarms));
        assert_eq!(of("ble-profile 5"), ("Bluetooth device 5".into(), Group::Events));
        assert_eq!(of("usb-conn"), ("USB connected".into(), Group::Events));
        let layer = l.events.iter().find(|e| e.name == "layer 4").unwrap();
        assert_eq!(layer.title(&[]), "Layer 4");
    }

    #[test]
    fn set_anim_raises_unused_timings() {
        let mut l = sample();
        let i = event(&l, "layer 0");
        l.events[i].set_anim(Anim::Blink);
        assert_eq!((l.events[i].blink_on_ms, l.events[i].blink_off_ms), (MS_MIN, MS_MIN));
        l.events[i].ease_in.ms = 4000;
        l.events[i].set_anim(Anim::Flash);
        assert_eq!(l.events[i].flash_ms, 4051);
    }

    #[test]
    fn diff_sends_only_changes() {
        let old = sample();
        assert!(diff(&old, &old).is_empty());

        let mut new = old.clone();
        new.on = true;
        new.brightness = Some(80);
        let i = event(&new, "layer 4");
        new.events[i].set_anim(Anim::Blink);
        new.events[i].colors = vec![Rgb { r: 1, g: 2, b: 3 }, Rgb { r: 4, g: 5, b: 6 }];
        new.events[i].pattern = vec![65, 25, 65];
        let i = event(&new, "usb-conn");
        new.events[i].flash_ms = 500;
        new.events[i].ease_out = Ease { ms: 100, function: "quad-out".into() };
        new.events[i].pattern.clear();
        assert_eq!(
            diff(&old, &new),
            [
                "argb on",
                "rtcfg set argb/brt 80",
                "argb evt usb-conn flash 500",
                "argb evt usb-conn flash-ease-out 100 quad-out",
                "argb evt usb-conn feedback 0",
                "argb evt layer 4 color 1 2 3",
                "argb evt layer 4 color 1 4 5 6",
                "argb evt layer 4 anim blink",
                "argb evt layer 4 blink 10 10",
                "argb evt layer 4 feedback 65 25 65",
            ]
        );
    }

    #[test]
    fn parses_layer_names() {
        assert_eq!(sample().layer_names, ["Default", "Extras", "Device", "Scroll", "Snipe", "User"]);
    }
}
