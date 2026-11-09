//! HID usage names (see assets/NOTICE) and ZMK's binding value layout:
//! `mods << 24 | page << 16 | id`, mods being implicit modifiers.

use std::sync::OnceLock;

pub struct Usage {
    pub page: u32,
    pub id: u32,
    pub short: &'static str,
    pub long: &'static str,
}

impl Usage {
    pub fn value(&self) -> u32 {
        join(0, self.page, self.id)
    }
}

/// Keyboard (page 7) and consumer (page 12) usages.
pub fn usages() -> &'static [Usage] {
    static TABLE: OnceLock<Vec<Usage>> = OnceLock::new();
    TABLE.get_or_init(|| {
        include_str!("../assets/hid_usages.tsv")
            .lines()
            .filter_map(|line| {
                let mut f = line.split('\t');
                Some(Usage {
                    page: f.next()?.parse().ok()?,
                    id: f.next()?.parse().ok()?,
                    short: f.next()?,
                    long: f.next()?,
                })
            })
            .collect()
    })
}

pub const MODS: [(u32, &str, &str); 8] = [
    (0x01, "LCtrl", "LC"),
    (0x02, "LShift", "LS"),
    (0x04, "LAlt", "LA"),
    (0x08, "LGUI", "LG"),
    (0x10, "RCtrl", "RC"),
    (0x20, "RShift", "RS"),
    (0x40, "RAlt", "RA"),
    (0x80, "RGUI", "RG"),
];

pub fn split(value: u32) -> (u32, u32, u32) {
    (value >> 24, (value >> 16) & 0xff, value & 0xffff)
}

pub fn join(mods: u32, page: u32, id: u32) -> u32 {
    mods << 24 | page << 16 | id
}

/// Key cap text in ZMK notation, e.g. "A", "LS(A)", "🔊".
pub fn label(value: u32) -> String {
    let (mods, page, id) = split(value);
    let base = usages()
        .iter()
        .find(|u| u.page == page && u.id == id)
        .map_or_else(|| format!("{page:#x}:{id:#x}"), |u| u.short.to_string());
    MODS.iter().rev().filter(|(bit, ..)| mods & bit != 0).fold(base, |acc, (_, _, abbr)| format!("{abbr}({acc})"))
}

/// Short behavior name for key caps (ZMK Studio's table, else word-truncated to 9 chars).
pub fn behavior_short(display_name: &str) -> String {
    const MAX: usize = 9;
    let known = include_str!("../assets/behavior_short_names.tsv")
        .lines()
        .find_map(|l| l.split_once('\t').filter(|(name, _)| *name == display_name).map(|(_, s)| s));
    if let Some(s) = known {
        return s.to_string();
    }
    if display_name.chars().count() <= MAX {
        return display_name.to_string();
    }
    let words: Vec<&str> = display_name.split_whitespace().collect();
    let per = (MAX / words.len().max(1)).max(1);
    words.iter().map(|w| w.chars().take(per).collect::<String>()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(label(0x0007_0004), "A");
        assert_eq!(label(0x0207_0004), "LS(A)");
        assert_eq!(label(0x0307_0004), "LC(LS(A))");
        assert_eq!(label(0x8007_0028), "RG(Ret)");
        assert_eq!(label(0x0001_0002), "0x1:0x2");
    }

    #[test]
    fn behavior_names() {
        assert_eq!(behavior_short("Momentary Layer"), "MLayer");
        assert_eq!(behavior_short("Key Press"), "");
        assert_eq!(behavior_short("Mouse Key Press"), "MouKeyPre");
    }
}
