//! Board status: battery level and the output the trackball currently sends its input to.
//!
//! Formats are from the board's shell source ("Battery: %u%%%s", "Output: USB|BLE|ESB"),
//! not yet recorded from a device.

use eg_shell::Shell;

use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Battery {
    pub percent: u8,
    /// The firmware's reading is not accurate while USB power is present.
    pub usb_powered: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Output {
    Usb,
    Ble,
    /// The 2.4 GHz dongle (ESB).
    Dongle,
}

/// A field is `None` if the firmware does not report it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Status {
    pub battery: Option<Battery>,
    pub output: Option<Output>,
}

/// Meant for polling, so it stays out of the shell log.
pub async fn status(shell: &Shell) -> Result<Status> {
    let status = shell.exec_bg("board status").await?;
    let output = shell.exec_bg("board output").await?;
    Ok(Status { battery: parse_battery(&status), output: parse_output(&output) })
}

/// "Battery: 87%" or "Battery: 87% (not accurate when USB power present)".
fn parse_battery(out: &str) -> Option<Battery> {
    let value = out.lines().find_map(|line| line.trim().strip_prefix("Battery:"))?;
    let (percent, note) = value.split_once('%')?;
    Some(Battery { percent: percent.trim().parse().ok()?, usb_powered: note.contains("USB power") })
}

/// "Output: USB", "Output: BLE", or "Output: ESB".
fn parse_output(out: &str) -> Option<Output> {
    match out.lines().find_map(|line| line.trim().strip_prefix("Output:"))?.trim() {
        "USB" => Some(Output::Usb),
        "BLE" => Some(Output::Ble),
        "ESB" => Some(Output::Dongle),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_battery() {
        let out = "Firmware: v1.6.2\nBattery: 87%\n";
        assert_eq!(parse_battery(out), Some(Battery { percent: 87, usb_powered: false }));
        let out = "Firmware: v1.6.2\nBattery: 100% (not accurate when USB power present)\n";
        assert_eq!(parse_battery(out), Some(Battery { percent: 100, usb_powered: true }));
        assert_eq!(parse_battery("board - Control the device"), None);
    }

    #[test]
    fn parses_output() {
        assert_eq!(parse_output("Output: USB"), Some(Output::Usb));
        assert_eq!(parse_output("Output: BLE\n"), Some(Output::Ble));
        assert_eq!(parse_output("Output: ESB"), Some(Output::Dongle));
        assert_eq!(parse_output(""), None);
    }
}
