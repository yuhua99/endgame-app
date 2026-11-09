//! Runtime config (`rtcfg`): named integer parameters with a default and a range,
//! registered by firmware modules and persisted by the firmware on `set`.

use eg_shell::Shell;

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// e.g. "p2sm/ema_alpha"; the part before the first '/' is the module.
    pub key: String,
    pub value: i32,
    pub default: i32,
    pub min: i32,
    pub max: i32,
}

impl Param {
    pub fn is_bool(&self) -> bool {
        self.min == 0 && self.max == 1
    }
}

pub async fn list(shell: &Shell) -> Result<Vec<Param>> {
    parse(&shell.exec("rtcfg list").await?)
}

/// Sets only the changed values, then reads back the device's list.
pub async fn apply(shell: &Shell, new: &[Param]) -> Result<Vec<Param>> {
    let current = list(shell).await?;
    for cmd in diff(&current, new) {
        run(shell, &cmd).await?;
    }
    list(shell).await
}

/// Restores the compiled default (the firmware also drops the stored value).
pub async fn reset(shell: &Shell, key: &str) -> Result<Vec<Param>> {
    run(shell, &format!("rtcfg reset {key}")).await?;
    list(shell).await
}

/// `set`/`reset` report failures (unknown key, out of range, storage error) as "Error: ...".
async fn run(shell: &Shell, cmd: &str) -> Result<()> {
    let out = shell.exec(cmd).await?;
    match out.lines().find(|l| l.trim_start().starts_with("Error")) {
        Some(line) => Err(Error::Rejected(format!("{cmd}: {}", line.trim()))),
        None => Ok(()),
    }
}

/// Lines like `  p2sm/ema_alpha   15  (default: 15, range: [1, 50])`.
pub fn parse(out: &str) -> Result<Vec<Param>> {
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            parse_line(line).ok_or_else(|| Error::Parse { cmd: "rtcfg list".into(), output: line.into() })
        })
        .collect()
}

pub fn diff(old: &[Param], new: &[Param]) -> Vec<String> {
    new.iter()
        .filter(|n| old.iter().any(|o| o.key == n.key && o.value != n.value))
        .map(|n| format!("rtcfg set {} {}", n.key, n.value))
        .collect()
}

fn parse_line(line: &str) -> Option<Param> {
    let (key, rest) = line.trim().split_once(char::is_whitespace)?;
    let (value, rest) = rest.trim_start().split_once(char::is_whitespace)?;
    let rest = rest.trim().strip_prefix("(default:")?.strip_suffix("])")?;
    let (default, range) = rest.split_once(", range: [")?;
    let (min, max) = range.split_once(',')?;
    Some(Param {
        key: key.to_string(),
        value: value.parse().ok()?,
        default: default.trim().parse().ok()?,
        min: min.trim().parse().ok()?,
        max: max.trim().parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = include_str!("../tests/fixtures/v1.6.2/rtcfg_list.txt");

    #[test]
    fn parses_recorded_list() {
        let params = parse(LIST).unwrap();
        assert_eq!(params.len(), LIST.lines().count());
        let alpha = params.iter().find(|p| p.key == "p2sm/ema_alpha").unwrap();
        assert_eq!(*alpha, Param { key: "p2sm/ema_alpha".into(), value: 15, default: 15, min: 1, max: 50 });
        let big = params.iter().find(|p| p.key == "accel/monitor_auto_off_ms").unwrap();
        assert_eq!(big.max, 3_600_000);
        assert!(params.iter().find(|p| p.key == "p2sm/frame_sync").unwrap().is_bool());
    }

    #[test]
    fn rejects_unknown_line() {
        assert!(matches!(parse("rtcfg: oops"), Err(Error::Parse { .. })));
    }

    #[test]
    fn diff_sets_changed_values_only() {
        let old = parse(LIST).unwrap();
        let mut new = old.clone();
        new.iter_mut().find(|p| p.key == "argb/brt").unwrap().value = 80;
        assert_eq!(diff(&old, &new), ["rtcfg set argb/brt 80"]);
    }
}
