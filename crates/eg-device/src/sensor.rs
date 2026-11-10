//! Sensor settings: pointer and twist-scroll sensitivity, twist scroll, SMA smoothing,
//! report sync window, frame sync, pointer plane rotation, and the behaviors that change
//! sensitivity from a key. Also the live surface quality readout.
//!
//! Commands and value formats follow Marshmellow UI. `p2sm status` and
//! `p2sm sens pointer get` outputs are recorded from firmware v1.6.2 (tests/fixtures);
//! `rrl get` ("Sync window: N") and `plane get pointer` ("angle=N") are taken from
//! Marshmellow's parsers and not yet recorded; so is `sensor surface`.

use std::ops::RangeInclusive;

use eg_shell::Shell;

use crate::{exec_checked, exec_optional, rtcfg, Error, Result};

/// Ranges used by Marshmellow UI's controls, in the units of the fields below.
pub const POINTER_RANGE: RangeInclusive<u32> = 10..=1000;
pub const TWIST_RANGE: RangeInclusive<u32> = 1..=1000;
pub const SMA_WINDOW_RANGE: RangeInclusive<u32> = 2..=12;
pub const SYNC_MS_RANGE: RangeInclusive<u32> = 0..=10;
/// Not taken from Marshmellow (it uses a free text field).
pub const PLANE_DEG_RANGE: RangeInclusive<i32> = -180..=180;
/// For `Behavior::step`, `min_step`, and `max_step`.
pub const STEP_RANGE: RangeInclusive<u32> = 1..=1000;
pub const MULTIPLIER_RANGE: RangeInclusive<u32> = 1..=5;
/// Not taken from Marshmellow: the firmware stores the duration in 16 bits and keeps at most
/// this many pattern values (its build default).
pub const FEEDBACK_MS_MAX: u32 = u16::MAX as u32;
pub const PATTERN_MAX: usize = 8;
const FRAME_SYNC: &str = "p2sm/frame_sync";

#[derive(Debug, Clone, PartialEq)]
pub struct SensorSettings {
    /// Pointer sensitivity in 0.1 % units (280 = 28 %).
    pub pointer: u32,
    pub twist_on: bool,
    pub twist_reversed: bool,
    /// Twist scroll sensitivity in 0.1 % units.
    pub twist: u32,
    pub sma_on: bool,
    /// SMA smoothing window in samples.
    pub sma_window: u32,
    /// Report rate limiter sync window in ms.
    pub sync_ms: u32,
    /// Pointer plane rotation in degrees; `None` if the firmware reports no angle.
    pub plane_deg: Option<i32>,
    /// Frame sync of the two sensors (an `rtcfg` parameter); `None` if the firmware lacks it.
    pub frame_sync: Option<bool>,
    pub behaviors: Vec<Behavior>,
}

/// A behavior that raises or lowers a sensitivity each time its key is pressed.
#[derive(Debug, Clone, PartialEq)]
pub struct Behavior {
    pub id: u32,
    pub name: String,
    /// Changes the twist scroll sensitivity instead of the pointer sensitivity.
    pub scroll: bool,
    /// Change per press in 0.1 % units.
    pub step: u32,
    /// Lowest and highest sensitivity, in steps. The firmware needs `min_step < max_step`.
    pub min_step: u32,
    pub max_step: u32,
    /// Highest sensitivity as a factor, whatever `max_step` allows. Used for scroll only.
    pub max_multiplier: u32,
    /// A press past one end continues from the other end.
    pub wrap: bool,
    pub feedback_on_limit: bool,
    /// Vibration per press in ms; 0 turns all vibration of this behavior off.
    pub feedback_duration: u32,
    /// Vibration played instead when the sensitivity wraps or stops at an end: ms on, off,
    /// on, ... The firmware reports it only with `wrap` or `feedback_on_limit`, so it is
    /// empty otherwise, and applying an edit then clears the pattern on the device.
    pub pattern: Vec<i32>,
}

pub async fn read(shell: &Shell) -> Result<SensorSettings> {
    let status = shell.exec("p2sm status").await?;
    let pointer = shell.exec("p2sm sens pointer get").await?;
    let rrl = shell.exec("rrl get").await?;
    let plane = exec_optional(shell, "plane get pointer").await?;
    let frame_sync = rtcfg::get(shell, FRAME_SYNC).await?.map(|v| v != 0);
    Ok(SensorSettings { frame_sync, ..parse(&status, &pointer, &rrl, plane.as_deref())? })
}

/// Sends the settings that differ between `saved` (what the edits started from) and `new`,
/// then reads back what the device actually has. Settings changed elsewhere meanwhile are kept.
pub async fn apply(shell: &Shell, saved: &SensorSettings, new: &SensorSettings) -> Result<SensorSettings> {
    let mut from = saved.clone();
    if new.twist_reversed != saved.twist_reversed {
        // `p2sm twist reverse` toggles, so it has to start from the device's current state.
        from.twist_reversed = read(shell).await?.twist_reversed;
    }
    for cmd in diff(&from, new) {
        exec_checked(shell, &cmd).await?;
    }
    read(shell).await
}

/// One sensor's surface tracking quality, out of `max`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Surface {
    pub sensor: u32,
    pub quality: u32,
    pub max: u32,
}

/// Current surface quality of each sensor. Firmware without the command fails with
/// `Unsupported` (see `Error::is_unsupported`). Meant for polling, so it stays out of the shell log.
pub async fn surface(shell: &Shell) -> Result<Vec<Surface>> {
    parse_surface(&shell.exec_bg("sensor surface").await?)
}

/// One "Sensor #1: surface quality = 412/728" line per sensor, in any letter case.
pub fn parse_surface(out: &str) -> Result<Vec<Surface>> {
    let number = |v: &str| v.trim_start().split(|c: char| !c.is_ascii_digit()).next()?.parse().ok();
    let readings: Vec<Surface> = out
        .lines()
        .filter_map(|line| {
            let line = line.to_ascii_lowercase();
            let (sensor, rest) = line[line.find("sensor #")? + "sensor #".len()..].split_once(':')?;
            let (quality, max) =
                rest.trim_start().strip_prefix("surface quality")?.split_once('=')?.1.split_once('/')?;
            Some(Surface { sensor: number(sensor)?, quality: number(quality)?, max: number(max)? })
        })
        .collect();
    if readings.is_empty() {
        return Err(Error::Parse { cmd: "sensor surface".into(), output: out.into() });
    }
    Ok(readings)
}

/// The "[ID 0] Name [scroll]" blocks of `p2sm status`; `None` if one is not understood.
fn parse_behaviors(status: &str) -> Option<Vec<Behavior>> {
    let blocks = status.split("\n[ID ").skip(1);
    blocks
        .map(|block| {
            let (id, rest) = block.split_once(']')?;
            let header = rest.lines().next()?.trim();
            let (name, scroll) = match header.strip_suffix("[scroll]") {
                Some(name) => (name.trim_end(), true),
                None => (header, false),
            };
            let number = |key: &str| fields(block, key).next()?.parse().ok();
            let flag = |key: &str| fields(block, key).next()?.parse().ok();
            // "min_step: 2, max_step: 12"
            let (min_step, max_step) = fields(block, "min_step:").next()?.split_once(", max_step:")?;
            let pattern = match fields(block, "feedback_wrap_pattern:").next() {
                Some(list) => {
                    let values = list.trim_matches(['[', ']']).split(',');
                    values.map(|v| v.trim().parse().ok()).collect::<Option<_>>()?
                }
                None => Vec::new(),
            };
            Some(Behavior {
                id: id.trim().parse().ok()?,
                name: name.to_string(),
                scroll,
                step: number("step:")?,
                min_step: min_step.trim().parse().ok()?,
                max_step: max_step.trim().parse().ok()?,
                max_multiplier: number("max_multiplier:")?,
                wrap: flag("wrap:")?,
                feedback_on_limit: flag("feedback_on_limit:")?,
                feedback_duration: number("feedback_duration:")?,
                pattern,
            })
        })
        .collect()
}

/// Everything but `frame_sync`, which `read` gets from `rtcfg`.
pub fn parse(status: &str, pointer: &str, rrl: &str, plane: Option<&str>) -> Result<SensorSettings> {
    let bad = |cmd: &str, output: &str| Error::Parse { cmd: cmd.into(), output: output.into() };
    let flag = |key: &str, on: &str, off: &str| {
        fields(status, key).find_map(|v| match v {
            v if v == on => Some(true),
            v if v == off => Some(false),
            _ => None,
        })
    };
    // "Twist scroll:" appears twice: "enabled" under General and "32%" under Sensitivity.
    let pct = |key: &str| fields(status, key).find_map(parse_pct);

    Ok(SensorSettings {
        pointer: pointer
            .split_whitespace()
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| bad("p2sm sens pointer get", pointer))?,
        twist_on: flag("Twist scroll:", "enabled", "disabled").ok_or_else(|| bad("p2sm status", status))?,
        twist_reversed: flag("Twist reversed:", "yes", "no").ok_or_else(|| bad("p2sm status", status))?,
        twist: pct("Twist scroll:").ok_or_else(|| bad("p2sm status", status))?,
        sma_on: flag("SMA smoothing:", "enabled", "disabled").ok_or_else(|| bad("p2sm status", status))?,
        sma_window: fields(status, "SMA window:")
            .find_map(|v| v.parse().ok())
            .ok_or_else(|| bad("p2sm status", status))?,
        sync_ms: fields(rrl, "Sync window:")
            .find_map(|v| v.split_whitespace().next()?.parse().ok())
            .ok_or_else(|| bad("rrl get", rrl))?,
        plane_deg: plane.and_then(parse_angle),
        frame_sync: None,
        behaviors: parse_behaviors(status).ok_or_else(|| bad("p2sm status", status))?,
    })
}

/// Commands that turn `old` into `new`. `p2sm twist reverse` toggles, so
/// `old.twist_reversed` must be the device's current state.
pub fn diff(old: &SensorSettings, new: &SensorSettings) -> Vec<String> {
    let on_off = |on: bool| if on { "on" } else { "off" };
    let mut cmds = Vec::new();
    if new.pointer != old.pointer {
        cmds.push(format!("p2sm sens pointer set {}", new.pointer));
    }
    if new.twist_on != old.twist_on {
        cmds.push(format!("p2sm twist {}", on_off(new.twist_on)));
    }
    if new.twist != old.twist {
        cmds.push(format!("p2sm sens twist set {}", new.twist));
    }
    if new.twist_reversed != old.twist_reversed {
        cmds.push("p2sm twist reverse".into());
    }
    if new.sma_window != old.sma_window {
        cmds.push(format!("p2sm sma window set {}", new.sma_window));
    }
    if new.sma_on != old.sma_on {
        cmds.push(format!("p2sm sma {}", on_off(new.sma_on)));
    }
    if new.sync_ms != old.sync_ms {
        cmds.push(format!("rrl set {}", new.sync_ms));
    }
    if let Some(deg) = new.plane_deg.filter(|_| new.plane_deg != old.plane_deg) {
        cmds.push(format!("plane set pointer {deg}"));
    }
    if let Some(on) = new.frame_sync.filter(|_| new.frame_sync != old.frame_sync) {
        cmds.push(format!("rtcfg set {FRAME_SYNC} {}", on as u8));
    }
    for b in &new.behaviors {
        if old.behaviors.iter().find(|o| o.id == b.id) != Some(b) {
            // id, step, min, max, multiplier, wrap, feedback on limit, duration, pattern length, pattern.
            let pattern: String = b.pattern.iter().map(|v| format!(" {v}")).collect();
            cmds.push(format!(
                "p2sm behavior set {} {} {} {} {} {} {} {} {}{pattern}",
                b.id,
                b.step,
                b.min_step,
                b.max_step,
                b.max_multiplier,
                b.wrap as u8,
                b.feedback_on_limit as u8,
                b.feedback_duration,
                b.pattern.len()
            ));
        }
    }
    cmds
}

/// Values of lines starting with `key`, e.g. "Pointer: 28%" -> "28%".
fn fields<'a>(out: &'a str, key: &'a str) -> impl Iterator<Item = &'a str> {
    out.lines().filter_map(move |line| line.trim().strip_prefix(key)).map(str::trim)
}

/// "32%" or "~32.5%" -> 325 (0.1 % units).
fn parse_pct(v: &str) -> Option<u32> {
    let pct: f64 = v.strip_suffix('%')?.trim_start_matches('~').parse().ok()?;
    Some((pct * 10.0).round() as u32)
}

/// "... angle=-15 ..." -> -15.
fn parse_angle(out: &str) -> Option<i32> {
    let rest = &out[out.find("angle=")? + "angle=".len()..];
    let end = rest
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && c == '-')))
        .map_or(rest.len(), |(i, _)| i);
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = include_str!("../tests/fixtures/v1.6.2/p2sm_status.txt");
    const POINTER: &str = include_str!("../tests/fixtures/v1.6.2/p2sm_sens_pointer_get.txt");

    fn sample() -> SensorSettings {
        parse(STATUS, POINTER, "Sync window: 4", Some("pointer: angle=-15")).unwrap()
    }

    #[test]
    fn parses_recorded_output() {
        let sample = sample();
        assert_eq!(
            SensorSettings { behaviors: Vec::new(), ..sample.clone() },
            SensorSettings {
                pointer: 280,
                twist_on: true,
                twist_reversed: false,
                twist: 320,
                sma_on: false,
                sma_window: 3,
                sync_ms: 4,
                plane_deg: Some(-15),
                frame_sync: None,
                behaviors: Vec::new(),
            }
        );
        assert_eq!(sample.behaviors.len(), 4);
        assert_eq!(
            sample.behaviors[0],
            Behavior {
                id: 0,
                name: "Scroll sensitivity (no wrapping)".into(),
                scroll: true,
                step: 10,
                min_step: 2,
                max_step: 12,
                max_multiplier: 1,
                wrap: false,
                feedback_on_limit: false,
                feedback_duration: 65,
                pattern: Vec::new(),
            }
        );
        assert_eq!(
            sample.behaviors[3],
            Behavior {
                id: 3,
                name: "Pointer sensitivity".into(),
                scroll: false,
                step: 125,
                min_step: 1,
                max_step: 1000,
                max_multiplier: 1,
                wrap: true,
                feedback_on_limit: false,
                feedback_duration: 65,
                pattern: vec![0, 10, 65, 25, 65, 25, 65],
            }
        );
    }

    #[test]
    fn status_without_behaviors_has_none() {
        let status = STATUS.split("\n[ID ").next().unwrap();
        assert!(parse(status, POINTER, "Sync window: 4", None).unwrap().behaviors.is_empty());
    }

    #[test]
    fn malformed_behavior_is_a_parse_error() {
        let status = STATUS.replace("wrap: true", "wrap: maybe");
        let err = parse(&status, POINTER, "Sync window: 4", None).unwrap_err();
        assert!(matches!(err, Error::Parse { cmd, .. } if cmd == "p2sm status"));
    }

    #[test]
    fn diff_sets_only_edited_behaviors() {
        let old = sample();
        let mut new = old.clone();
        new.behaviors[1].step = 100;
        new.behaviors[2].wrap = false;
        new.behaviors[2].pattern = vec![0, 10, 65];
        assert_eq!(
            diff(&old, &new),
            ["p2sm behavior set 1 100 1 8 1 0 0 65 0", "p2sm behavior set 2 250 1 1000 1 0 0 65 3 0 10 65"]
        );
    }

    #[test]
    fn plane_is_optional() {
        assert_eq!(parse(STATUS, POINTER, "Sync window: 4", None).unwrap().plane_deg, None);
        assert_eq!(parse(STATUS, POINTER, "Sync window: 4", Some("no such device")).unwrap().plane_deg, None);
    }

    #[test]
    fn reports_which_command_failed_to_parse() {
        let err = parse(STATUS, POINTER, "garbage", None).unwrap_err();
        assert!(matches!(err, Error::Parse { cmd, .. } if cmd == "rrl get"));
    }

    #[test]
    fn parses_decimal_and_approx_pct() {
        assert_eq!(parse_pct("~32.5%"), Some(325));
        assert_eq!(parse_pct("enabled"), None);
    }

    #[test]
    fn parses_surface_quality() {
        let out = "Sensor #1: surface quality = 412/728\nSensor #2: surface quality=0/728 (lifted)\n";
        assert_eq!(
            parse_surface(out).unwrap(),
            [Surface { sensor: 1, quality: 412, max: 728 }, Surface { sensor: 2, quality: 0, max: 728 }]
        );
        assert_eq!(
            parse_surface("sensor #3: SURFACE QUALITY = 5/9").unwrap(),
            [Surface { sensor: 3, quality: 5, max: 9 }]
        );
        let err = parse_surface("Sensor #1: movement = 412/728").unwrap_err();
        assert!(matches!(err, Error::Parse { cmd, .. } if cmd == "sensor surface"));
    }

    #[test]
    fn diff_sends_only_changes() {
        let old = sample();
        assert!(diff(&old, &old).is_empty());

        let new =
            SensorSettings { pointer: 300, twist_reversed: true, sma_on: true, plane_deg: Some(0), ..old.clone() };
        assert_eq!(
            diff(&old, &new),
            ["p2sm sens pointer set 300", "p2sm twist reverse", "p2sm sma on", "plane set pointer 0"]
        );
    }

    #[test]
    fn diff_sets_frame_sync() {
        let old = SensorSettings { frame_sync: Some(true), ..sample() };
        let new = SensorSettings { frame_sync: Some(false), ..old.clone() };
        assert_eq!(diff(&old, &new), ["rtcfg set p2sm/frame_sync 0"]);
        assert!(diff(&old, &SensorSettings { frame_sync: None, ..old.clone() }).is_empty());
    }

    #[test]
    fn diff_never_sets_missing_plane() {
        let old = SensorSettings { plane_deg: Some(10), ..sample() };
        let new = SensorSettings { plane_deg: None, ..old.clone() };
        assert!(diff(&old, &new).is_empty());
    }
}
