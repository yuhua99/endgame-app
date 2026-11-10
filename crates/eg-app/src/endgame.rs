//! Top view of the Endgame trackball for the Keymap tab, used instead of ZMK-style boxes when
//! the device reports the Endgame physical layout. Key caps are lines, arcs around the ball,
//! and corner fillets fitted to the Marshmellow UI render (see assets/NOTICE); units are that
//! image's pixels. The right half mirrors the left.

use dioxus::prelude::*;
use eg_device::keymap::KeyPhysicalAttrs;

use crate::keymap::Cap;

/// `(width, height, x, y)` of each key in the firmware's `physical_layout0`, in position order.
const LAYOUT: [(i32, i32, i32, i32); 12] = [
    (200, 150, 275, 0),
    (200, 150, 525, 0),
    (150, 375, 75, 25),
    (150, 375, 775, 25),
    (150, 375, 75, 450),
    (150, 375, 775, 450),
    (200, 150, 275, 700),
    (200, 150, 525, 700),
    (35, 100, 15, 750),
    (35, 100, 950, 750),
    (100, 35, 65, 850),
    (100, 35, 835, 850),
];

pub fn matches(keys: &[KeyPhysicalAttrs]) -> bool {
    keys.len() == LAYOUT.len() && keys.iter().zip(LAYOUT).all(|(k, l)| (k.width, k.height, k.x, k.y) == l)
}

const TOP: &str = "M 184.6 92.8 A 12 12 0 0 1 196 77 L 360 77 A 8 8 0 0 1 368 85 L 368 191.8 Q 368 197.8 362 198.2 A 205 205 0 0 0 242.8 248.9 Q 238.3 252.9 236.4 247.2 L 184.6 92.8 Z";
const SIDE_UPPER: &str = "M 92 163 A 45.5 45.5 0 0 1 180.4 147.7 L 222.9 266.9 Q 224.6 271.7 221.4 275.5 A 202 202 0 0 0 176.7 393 Q 176.5 399 170.5 399 L 100 399 A 8 8 0 0 1 92 391 L 92 163 Z";
const SIDE_LOWER: &str = "M 92 429 A 8 8 0 0 1 100 421 L 166.3 421 Q 172.3 421 173 427 A 207 207 0 0 0 232.4 549.1 Q 236.7 553.3 232.3 557.4 L 162.5 621.9 A 42 42 0 0 1 92 591 L 92 429 Z";
const BOTTOM: &str = "M 251.2 568.7 Q 255 565.5 259.1 568.5 A 204.5 204.5 0 0 0 362 606.3 Q 368 606.7 368 612.7 L 368 670 A 8 8 0 0 1 360 678 L 202.3 678 A 30 30 0 0 1 183.2 624.9 L 251.2 568.7 Z";
/// Left and right halves of an encoder pill centered at x = 0.
const ENC_CCW: &str = "M -2 760 L -123 760 A 30 30 0 0 0 -123 820 L -2 820 Z";
const ENC_CW: &str = "M 2 760 L 123 760 A 30 30 0 0 1 123 820 L 2 820 Z";
/// Mirrors the left-half shapes onto the right half.
const MIRROR: &str = "translate(757,0) scale(-1,1)";

struct Shape {
    d: &'static str,
    transform: Option<String>,
    /// Label center.
    at: (i32, i32),
    /// Encoder direction arrow shown before the header.
    arrow: &'static str,
}

/// Shape of each key position. Encoders (`sensor-bindings = <&rotenc_follower 10 8
/// &rotenc_follower 9 11>`, clockwise first): left 10 cw / 8 ccw, right 9 cw / 11 ccw.
fn shape(pos: usize) -> Option<Shape> {
    let key = |d, mirror: bool, at| Shape { d, transform: mirror.then(|| MIRROR.to_string()), at, arrow: "" };
    let enc = |d, cx: i32, arrow| Shape { d, transform: Some(format!("translate({cx},0)")), at: (cx, 790), arrow };
    Some(match pos {
        0 => key(TOP, false, (275, 150)),
        1 => key(TOP, true, (482, 150)),
        2 => key(SIDE_UPPER, false, (135, 265)),
        3 => key(SIDE_UPPER, true, (622, 265)),
        4 => key(SIDE_LOWER, false, (138, 510)),
        5 => key(SIDE_LOWER, true, (619, 510)),
        6 => key(BOTTOM, false, (282, 640)),
        7 => key(BOTTOM, true, (475, 640)),
        8 => Shape { at: (142, 790), ..enc(ENC_CCW, 219, "↺ ") },
        10 => Shape { at: (296, 790), ..enc(ENC_CW, 219, "↻ ") },
        11 => Shape { at: (461, 790), ..enc(ENC_CCW, 538, "↺ ") },
        9 => Shape { at: (615, 790), ..enc(ENC_CW, 538, "↻ ") },
        _ => return None,
    })
}

const BODY: &str = r##"<rect x="64" y="19" width="629" height="711" rx="95" fill="#2a2d33" stroke="#454a53" stroke-width="2"/>
<rect x="80" y="55" width="597" height="645" rx="70" fill="#1f2126"/>
<circle cx="378.5" cy="402.5" r="191" fill="#2a2d33" stroke="#454a53" stroke-width="2"/>
<text class="caption" x="219" y="750">Encoder</text><text class="caption" x="538" y="750">Encoder</text>"##;

#[component]
pub fn EndgameLayout(caps: Vec<Cap>, selected: Option<usize>, on_select: EventHandler<usize>) -> Element {
    rsx! {
        // Top-aligned, so a tall tab leaves the spare room below the drawing.
        svg { class: "endgame", view_box: "0 0 757 840", preserve_aspect_ratio: "xMidYMin meet",
            g { dangerous_inner_html: BODY }
            for cap in caps {
                if let Some(s) = shape(cap.pos) {
                    g { key: "{cap.pos}", onclick: move |_| on_select.call(cap.pos),
                        path {
                            class: if selected == Some(cap.pos) { "ekey selected" } else { "ekey" },
                            d: s.d,
                            transform: s.transform,
                        }
                        text { class: "hdr", x: s.at.0, y: s.at.1 - 10, "{s.arrow}{cap.header}" }
                        text { class: "lbl", x: s.at.0, y: s.at.1 + 16, "{cap.label}" }
                    }
                }
            }
        }
    }
}
