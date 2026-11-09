//! Splits shell output into responses at each prompt.
//!
//! Observed device output for one command (BLE, firmware v1.6.2):
//! `\r\n<body lines ending \r\n>\r\n\x1b[1;32mendgame$ \x1b[0m`

/// The colored prompt, so a plain "endgame$ " inside output or logs does not end a response.
const PROMPT: &[u8] = b"\x1b[1;32mendgame$ ";

#[derive(Default)]
pub(crate) struct Framer {
    buf: Vec<u8>,
}

impl Framer {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Next complete response (everything before a prompt), without ANSI codes, CRs,
    /// or surrounding blank lines.
    pub(crate) fn pop(&mut self) -> Option<String> {
        let at = self.buf.windows(PROMPT.len()).position(|w| w == PROMPT)?;
        let raw: Vec<u8> = self.buf.drain(..at + PROMPT.len()).take(at).collect();
        let text = String::from_utf8_lossy(&strip_ansi(&raw)).replace('\r', "");
        Some(text.trim_matches('\n').to_string())
    }
}

/// Removes CSI sequences (`ESC [ params final`), e.g. the prompt colors.
fn strip_ansi(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == 0x1b && raw.get(i + 1) == Some(&b'[') {
            i += 2;
            while i < raw.len() && !(0x40..=0x7e).contains(&raw[i]) {
                i += 1;
            }
            i += 1;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: &str = "\x1b[1;32mendgame$ \x1b[0m";

    fn framer(chunks: &[&str]) -> Framer {
        let mut f = Framer::default();
        for c in chunks {
            f.push(c.as_bytes());
        }
        f
    }

    #[test]
    fn cleans_observed_response() {
        let mut f = framer(&[&format!("\r\n280 (28%)\r\n\r\n{P}")]);
        assert_eq!(f.pop().as_deref(), Some("280 (28%)"));
        assert_eq!(f.pop(), None);
    }

    #[test]
    fn keeps_leading_spaces_and_inner_lines() {
        let body = "  p2sm/ema_alpha   15  (default: 15, range: [1, 50])\r\n  ac/history_ttl   0  (default: 0, range: [0, 5000])";
        let mut f = framer(&[&format!("\r\n{body}\r\n\r\n{P}")]);
        assert_eq!(
            f.pop().as_deref(),
            Some("  p2sm/ema_alpha   15  (default: 15, range: [1, 50])\n  ac/history_ttl   0  (default: 0, range: [0, 5000])")
        );
    }

    #[test]
    fn waits_for_prompt_split_across_chunks() {
        let mut f = framer(&["\r\nFirmware: v1.6.2\r\n\r\n\x1b[1;32mend"]);
        assert_eq!(f.pop(), None);
        f.push(b"game$ \x1b[0m");
        assert_eq!(f.pop().as_deref(), Some("Firmware: v1.6.2"));
    }

    #[test]
    fn splits_two_responses_in_one_chunk() {
        let mut f = framer(&[&format!("\r\na\r\n{P}\r\nb\r\n{P}")]);
        assert_eq!(f.pop().as_deref(), Some("a"));
        assert_eq!(f.pop().as_deref(), Some("b"));
        assert_eq!(f.pop(), None);
    }

    #[test]
    fn strips_prompt_color_reset_arriving_with_next_response() {
        let mut f = framer(&["\r\na\r\n\x1b[1;32mendgame$ "]);
        assert_eq!(f.pop().as_deref(), Some("a"));
        f.push(format!("\x1b[0m\r\nb\r\n{P}").as_bytes());
        assert_eq!(f.pop().as_deref(), Some("b"));
    }

    #[test]
    fn plain_prompt_text_in_body_does_not_split() {
        let mut f = framer(&[&format!("\r\nsee endgame$ docs\r\n{P}")]);
        assert_eq!(f.pop().as_deref(), Some("see endgame$ docs"));
    }

    #[test]
    fn greeting_prompt_is_empty_response() {
        let mut f = framer(&[P]);
        assert_eq!(f.pop().as_deref(), Some(""));
    }
}
