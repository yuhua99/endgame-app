//! Firmware versions: the installed one against a release tag.

/// The version of the release tagged `tag` ("endgame-1.6.3" -> "v1.6.3") if it is newer than
/// `installed` ("v1.6.2", as `board version` reports it).
pub fn newer(installed: &str, tag: &str) -> Option<String> {
    let mut installed = parse(installed)?;
    // PAW3395 builds report the major version plus 100 (per Marshmellow UI).
    if installed[0] >= 100 {
        installed[0] -= 100;
    }
    let [major, minor, patch] = parse(tag).filter(|latest| *latest > installed)?;
    Some(format!("v{major}.{minor}.{patch}"))
}

/// "v1.6.2" or "endgame-1.6.2" -> [1, 6, 2].
fn parse(version: &str) -> Option<[u32; 3]> {
    let digits = version.trim_start_matches(|c: char| !c.is_ascii_digit());
    let mut numbers = digits.split('.').map(|n| n.parse().ok());
    Some([numbers.next()??, numbers.next()??, numbers.next()??])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_newer_release() {
        assert_eq!(newer("v1.6.2", "endgame-1.6.3").as_deref(), Some("v1.6.3"));
        assert_eq!(newer("v1.6.2", "endgame-2.0.0").as_deref(), Some("v2.0.0"));
        assert_eq!(newer("1.0.1", "v1.6.2").as_deref(), Some("v1.6.2"));
        // Numbers, not text: 10 is after 9.
        assert_eq!(newer("v1.9.0", "endgame-1.10.0").as_deref(), Some("v1.10.0"));
    }

    #[test]
    fn ignores_the_same_or_an_older_release() {
        assert_eq!(newer("v1.6.2", "endgame-1.6.2"), None);
        assert_eq!(newer("v1.6.2", "endgame-1.5.9"), None);
        assert_eq!(newer("v2.0.0", "endgame-1.6.2"), None);
    }

    #[test]
    fn compares_paw3395_builds_without_the_offset() {
        assert_eq!(newer("v101.6.2", "endgame-1.6.2"), None);
        assert_eq!(newer("v101.6.1", "endgame-1.6.2").as_deref(), Some("v1.6.2"));
    }

    #[test]
    fn ignores_unrecognized_versions() {
        assert_eq!(newer("unknown", "endgame-1.6.3"), None);
        assert_eq!(newer("v1.6.2", "nightly"), None);
        assert_eq!(newer("v1.6.2", "endgame-1.7"), None);
    }
}
