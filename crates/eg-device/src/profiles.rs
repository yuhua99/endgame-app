//! Keymap profiles. A slot stores the keymap's differences from the compiled one together
//! with the OS mode. An output (USB, a Bluetooth profile, the dongle) can be bound to a
//! profile, which the firmware activates when that output becomes the active one.
//!
//! `keymap` formats are from the zmk-keymap-shell source and `bistable slot` ("slot: N")
//! from Marshmellow's parser; none is recorded from a device yet.

use eg_shell::Shell;

use crate::{exec_optional, keymap, rtcfg, Error, Result, Session};

/// Longest profile name the firmware can bind to an output.
pub const NAME_MAX: usize = 23;
const AUTOSWITCH: &str = "keymap/autoswitch";
const DEFAULT_OS: &str = "bst/default";

/// Which variant OS-dependent behaviors (such as copy and paste) use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Os {
    WinLinux,
    Mac,
}

impl Os {
    fn from_slot(slot: i32) -> Option<Os> {
        match slot {
            0 => Some(Os::WinLinux),
            1 => Some(Os::Mac),
            _ => None,
        }
    }

    fn slot(self) -> u8 {
        match self {
            Os::WinLinux => 0,
            Os::Mac => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    /// Counted from 1, as the firmware does.
    pub index: u32,
    /// `None` if nothing is stored. A profile stored without a name has an empty one.
    pub name: Option<String>,
    /// The current keymap and OS mode are this profile's.
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    /// "usb" or "wireless-N".
    pub output: String,
    /// Name of the bound profile.
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Profiles {
    pub slots: Vec<Slot>,
    /// The current keymap or OS mode differs from the firmware's default.
    pub customized: bool,
    /// Empty if the firmware cannot bind outputs to profiles.
    pub assignments: Vec<Assignment>,
    /// Whether an output change activates the bound profile; `None` if not supported.
    pub autoswitch: Option<bool>,
    pub os: Option<Os>,
    /// The OS mode of the default keymap and of profiles stored without one.
    pub default_os: Option<Os>,
}

/// `name` must satisfy `valid_name`.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Stores the current keymap in an empty slot.
    Save {
        slot: u32,
        name: String,
    },
    Overwrite {
        slot: u32,
        name: String,
    },
    Activate(u32),
    Delete(u32),
    /// Back to the compiled keymap and the default OS mode.
    RestoreDefault,
    /// Binds `output` to the profile in `slot`, or to none.
    Assign {
        output: String,
        slot: Option<u32>,
    },
    Autoswitch(bool),
    Os(Os),
    DefaultOs(Os),
}

impl Change {
    /// The command and a text its reply contains on success.
    fn command(&self) -> (String, &'static str) {
        match self {
            Change::Save { slot, name } => (format!("keymap save {slot} {name}"), "Saved:"),
            Change::Overwrite { slot, name } => (format!("keymap overwrite {slot} {name}"), "Saved:"),
            Change::Activate(slot) => (format!("keymap activate {slot}"), "Activated:"),
            Change::Delete(slot) => (format!("keymap destroy {slot}"), "Destroyed."),
            Change::RestoreDefault => ("keymap restore".into(), "Restored."),
            Change::Assign { output, slot: Some(slot) } => (format!("keymap assign {output} {slot}"), "Assigned"),
            Change::Assign { output, slot: None } => (format!("keymap assign {output}"), "Cleared"),
            Change::Autoswitch(on) => (format!("rtcfg set {AUTOSWITCH} {}", *on as u8), " = "),
            // The reply format is not known; the state read afterwards shows the result.
            Change::Os(os) => (format!("bistable set {}", os.slot()), ""),
            Change::DefaultOs(os) => (format!("rtcfg set {DEFAULT_OS} {}", os.slot()), " = "),
        }
    }
}

/// Names are single shell words, and an all-digit name would be read as a slot number.
pub fn valid_name(name: &str) -> bool {
    (1..=NAME_MAX).contains(&name.len())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !name.chars().all(|c| c.is_ascii_digit())
}

pub async fn read(shell: &Shell) -> Result<Profiles> {
    let (slots, customized) = parse_status(&shell.exec("keymap status").await?)?;
    let assignments = exec_optional(shell, "keymap assign").await?;
    let os = exec_optional(shell, "bistable slot").await?;
    Ok(Profiles {
        slots,
        customized,
        assignments: assignments.map(|out| parse_assignments(&out)).unwrap_or_default(),
        autoswitch: rtcfg::get(shell, AUTOSWITCH).await?.map(|v| v != 0),
        os: os.and_then(|out| parse_os(&out)),
        default_os: rtcfg::get(shell, DEFAULT_OS).await?.and_then(Os::from_slot),
    })
}

/// Applies one change; `read` then shows the result.
pub async fn change(session: &Session, change: &Change) -> Result<()> {
    let shell = session.shell();
    let (cmd, ok) = change.command();
    if matches!(change, Change::Save { .. } | Change::Overwrite { .. } | Change::Activate(_) | Change::RestoreDefault) {
        // Storing takes only what Studio has saved, and replacing the keymap drops the rest.
        // If Studio cannot tell (locked, no answer), the change is not made either.
        if let Some(studio) = session.studio() {
            if keymap::has_unsaved(studio).await? {
                return Err(Error::UnsavedKeymap);
            }
        }
    }
    if cmd.starts_with("keymap ") {
        // The firmware works on a snapshot that only `keymap status` refreshes.
        shell.exec("keymap status").await?;
    }
    let out = shell.exec(&cmd).await?;
    if !out.contains(ok) {
        let reply = out.split_whitespace().collect::<Vec<_>>().join(" ");
        return Err(Error::Rejected(format!("{cmd}: {reply}")));
    }
    Ok(())
}

/// Slot lines ` >Slot 1: 812 bytes, name "work"` (active), ` Slot 2: 96 bytes, name "game"`,
/// `  Slot 3: unoccupied`; "No changes detected." if the keymap is the firmware's default.
fn parse_status(out: &str) -> Result<(Vec<Slot>, bool)> {
    let slots: Vec<Slot> = out
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (index, stored) = line.trim_start_matches('>').trim_start().strip_prefix("Slot ")?.split_once(':')?;
            let name = match stored.trim() {
                "unoccupied" => None,
                stored => Some(stored.split('"').nth(1).filter(|n| *n != "(unnamed)").unwrap_or_default().to_string()),
            };
            Some(Slot { index: index.trim().parse().ok()?, name, active: line.starts_with('>') })
        })
        .collect();
    if slots.is_empty() {
        return Err(Error::Parse { cmd: "keymap status".into(), output: out.into() });
    }
    Ok((slots, !out.contains("No changes detected")))
}

/// Lines `  usb          work` and `  wireless-1   (none)`.
fn parse_assignments(out: &str) -> Vec<Assignment> {
    out.lines()
        .filter_map(|line| {
            let (output, profile) = line.trim().split_once(char::is_whitespace)?;
            let known = output == "usb" || output.strip_prefix("wireless-").is_some_and(|n| n.parse::<u32>().is_ok());
            let profile = profile.trim();
            known.then(|| Assignment {
                output: output.to_string(),
                profile: (profile != "(none)").then(|| profile.to_string()),
            })
        })
        .collect()
}

/// "slot: 1" in any letter case.
fn parse_os(out: &str) -> Option<Os> {
    let out = out.to_ascii_lowercase();
    let value = out[out.find("slot:")? + "slot:".len()..].trim_start();
    Os::from_slot(value.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status() {
        let out = " >Slot 1: 812 bytes, name \"work\"\n Slot 2: 96 bytes, name \"(unnamed)\"\n  Slot 3: unoccupied\n  Slot 4: unoccupied\n";
        let slot = |index, name: Option<&str>, active| Slot { index, name: name.map(String::from), active };
        assert_eq!(
            parse_status(out).unwrap(),
            (
                vec![slot(1, Some("work"), true), slot(2, Some(""), false), slot(3, None, false), slot(4, None, false)],
                true
            )
        );
        let out = "No changes detected.\n\n  Slot 1: unoccupied\n";
        assert_eq!(parse_status(out).unwrap(), (vec![slot(1, None, false)], false));
        assert!(matches!(parse_status("Not initialized!"), Err(Error::Parse { .. })));
    }

    #[test]
    fn parses_assignments() {
        let out = "Output assignments:\n  usb          work\n  wireless-1   (none)\n";
        assert_eq!(
            parse_assignments(out),
            [
                Assignment { output: "usb".into(), profile: Some("work".into()) },
                Assignment { output: "wireless-1".into(), profile: None },
            ]
        );
        assert!(parse_assignments("keymap - Keymap management").is_empty());
    }

    #[test]
    fn parses_os() {
        assert_eq!(parse_os("slot: 0"), Some(Os::WinLinux));
        assert_eq!(parse_os("Current slot: 1\n"), Some(Os::Mac));
        assert_eq!(parse_os("bistable - Bistable behavior control"), None);
    }

    #[test]
    fn builds_commands() {
        let save = Change::Save { slot: 2, name: "work".into() };
        assert_eq!(save.command(), ("keymap save 2 work".to_string(), "Saved:"));
        let clear = Change::Assign { output: "wireless-1".into(), slot: None };
        assert_eq!(clear.command().0, "keymap assign wireless-1");
        assert_eq!(Change::Os(Os::Mac).command().0, "bistable set 1");
        assert_eq!(Change::Autoswitch(false).command().0, "rtcfg set keymap/autoswitch 0");
    }

    #[test]
    fn validates_names() {
        assert!(valid_name("left_hand-2"));
        assert!(!valid_name(""));
        assert!(!valid_name("two words"));
        assert!(!valid_name("12"));
        assert!(!valid_name(&"a".repeat(NAME_MAX + 1)));
    }
}
