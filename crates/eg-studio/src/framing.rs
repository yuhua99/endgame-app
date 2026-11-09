//! Studio frames: `SOF payload EOF`; payload bytes equal to SOF/ESC/EOF are preceded by ESC
//! (sent as-is, no XOR). Matches zmk-studio-ts-client framing.ts.

const SOF: u8 = 0xAB;
const ESC: u8 = 0xAC;
const EOF: u8 = 0xAD;

pub(crate) fn encode(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 2);
    out.push(SOF);
    for &b in payload {
        if matches!(b, SOF | ESC | EOF) {
            out.push(ESC);
        }
        out.push(b);
    }
    out.push(EOF);
    out
}

/// Streaming decoder; frames may span or share chunks. Malformed input is skipped up to
/// the next SOF instead of failing the connection.
#[derive(Default)]
pub(crate) struct Decoder {
    buf: Vec<u8>,
    in_frame: bool,
    escaped: bool,
}

impl Decoder {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        for &b in bytes {
            if self.escaped {
                self.buf.push(b);
                self.escaped = false;
                continue;
            }
            match (self.in_frame, b) {
                (_, SOF) => {
                    if self.in_frame {
                        log::warn!("studio: SOF inside frame, dropping {} bytes", self.buf.len());
                    }
                    self.buf.clear();
                    self.in_frame = true;
                }
                (false, _) => log::warn!("studio: byte {b:#04x} outside frame"),
                (true, ESC) => self.escaped = true,
                (true, EOF) => {
                    frames.push(std::mem::take(&mut self.buf));
                    self.in_frame = false;
                }
                (true, _) => self.buf.push(b),
            }
        }
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_control_bytes() {
        assert_eq!(encode(&[0x01, 0xAB]), [0xAB, 0x01, 0xAC, 0xAB, 0xAD]);
        assert_eq!(encode(&[0xAC, 0xAD]), [0xAB, 0xAC, 0xAC, 0xAC, 0xAD, 0xAD]);
    }

    #[test]
    fn round_trips_across_chunks() {
        let payload = [0x00, 0xAB, 0x10, 0xAC, 0xAD, 0xFF];
        let wire = encode(&payload);
        let mut d = Decoder::default();
        let mut frames = Vec::new();
        for b in &wire {
            frames.extend(d.push(&[*b]));
        }
        assert_eq!(frames, [payload.to_vec()]);
    }

    #[test]
    fn splits_frames_in_one_chunk_and_skips_garbage() {
        let mut wire = vec![0x42];
        wire.extend(encode(&[1]));
        wire.extend(encode(&[2, 3]));
        assert_eq!(Decoder::default().push(&wire), [vec![1], vec![2, 3]]);
    }

    #[test]
    fn restarts_on_sof_mid_frame() {
        let mut d = Decoder::default();
        assert!(d.push(&[0xAB, 1, 2]).is_empty());
        assert_eq!(d.push(&[0xAB, 3, 0xAD]), [vec![3]]);
    }
}
