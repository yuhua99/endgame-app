# Endgame App — Architecture

Desktop app for configuring the efog.tech Endgame trackball (ZMK firmware) over BLE, USB, or the 1k dongle.
Goal: cover all Marshmellow UI features plus ZMK-Studio-style key remapping.

Stack: Rust only. UI uses Dioxus desktop (native Rust process rendering to a webview via wry). No Tauri, no IPC, no TypeScript.

## Device protocols

The device exposes up to three channels; which ones are available depends on how it is connected.

| Connection | shell (text) | studio (protobuf RPC) | data (stream) |
|---|---|---|---|
| BLE direct | GATT svc `c901c4e9-5770-4bf1-96b2-2dd287813e6e`, chrc `c901c4ea-…` (write-without-resp + notify) | GATT svc `00000000-0196-6107-c967-c5cfb1c2482a`, chrc `00000001-0196-6107-c967-c5cfb1c2482a` | chrc `c901c4eb-…` (notify) |
| USB direct (VID 0x0011, PID 6) | CDC ACM serial | CDC ACM serial (separate port? unverified) | same as shell |
| Dongle (VID 0x0011, PID 7) | CDC ACM, relayed over ESB | not supported | not supported |

### Shell (Zephyr shell)

- Facts reverse-engineered from `Marshmellow.html` and the efogdev firmware modules.
- Command = UTF-8 line ending in `\n`. Response = everything before the next prompt.
- Prompts: `endgame$ `, `uart:~$ `, `zmk$`, `zmk:~$`, possibly wrapped in ANSI color codes. Strip ANSI, then prompt.
- After connect: `shell echo off`. Probe features by checking for `command not found` (`rtcfg --help`, `argb evt`).
- BLE writes: chunks ≤ 96 B, 20 ms apart. Serial: 460800 baud.
- Timeouts used by Marshmellow: 5 s idle, 9.5 s total.
- Dongle relay session must be requested on the device; it ends after 60 s of inactivity → keep alive with `__ping`.
- Command families: `board` (status/version/output/backup/restore/layers/rgb), `rtcfg` (list/get/set/reset), `keymap` (status/save/overwrite/activate/destroy/restore/assign), `p2sm`, `rrl`, `plane`, `curve`, `argb`, `bistable`, `sensor` (stream/surface), `ec11`.
- `rtcfg list` line format: `<key> <value> (default: <d>, range: [<min>, <max>])`.

### Studio (ZMK Studio RPC)

- Protobuf messages from `zmkfirmware/zmk-studio-messages`, framed with SOF/ESC/EOF bytes.
- Keymap edits require the device to be unlocked (`&studio_unlock` key; `CONFIG_ZMK_STUDIO_LOCKING=y`). Firmware `board backup/restore` also checks the lock.
- Reference implementation: `../zmk-studio` (Tauri + React, Apache-2.0). Its native BLE discovery lists devices that are already connected; Web Bluetooth on Windows/macOS cannot.

## Crates

```
                    eg-app (bin, Dioxus desktop)
                   /            |             \
           eg-device        eg-firmware      dioxus
          /    |     \
   eg-shell eg-studio eg-transport
        \      |      /
      tokio::mpsc (Link = a pair of channels)
```

Only `eg-app` depends on Dioxus. `eg-shell` / `eg-studio` do not depend on `eg-transport`; they take a `Link`, so tests feed them plain channels.

```
endgame-app/
  Cargo.toml                 [workspace]
  crates/
    eg-transport/  src/{lib, ble, serial, discovery}.rs
    eg-shell/      src/{lib, framer, client}.rs
    eg-studio/     src/{lib, framing, client}.rs, build.rs (prost), proto/ (zmk-studio-messages)
    eg-device/     src/{lib, session, caps}.rs
                   src/features/{sensor, rtcfg, keymap_slots, output, backup, lighting,
                                 bistable, curves, encoder, monitor, keymap}.rs
                   tests/fixtures/*.txt        recorded device output
    eg-firmware/   src/{lib, release, uf2}.rs
    eg-app/        src/{main, state, registry}.rs
                   src/pages/{connect, sensor, rtcfg, ..., keymap_editor}.rs
                   src/widgets/{physical_layout, behavior_picker, hid_usage_picker}.rs
                   assets/{hid-usages.json, style.css}   ported from zmk-studio, keep NOTICE
```

### eg-transport

The only crate that touches the OS / hardware.

```rust
pub struct Link { pub tx: mpsc::Sender<Vec<u8>>, pub rx: mpsc::UnboundedReceiver<Vec<u8>> } // rx ends = disconnected
pub struct DeviceInfo { pub id: String, pub name: String, pub kind: Kind }        // Ble | Usb | Dongle
pub async fn scan() -> Result<Vec<DeviceInfo>>;
pub async fn open(dev: &DeviceInfo) -> Result<Connection>;
impl Connection {
    async fn shell(&self) -> Result<Link>; // subscribes before returning
    fn studio(&self) -> Option<Result<Link>>;
    fn data(&self) -> Option<Result<Link>>;
}
```

- One BLE connection serves several characteristic links (ref-counted).
- Hides: BLE chunking (96 B / 20 ms), macOS connected-device quirks, serial VID/PID → port mapping.
- BLE via `bluest` (same as zmk-studio).

### eg-shell

```rust
impl Shell {                                                   // Clone (Arc inside)
    pub async fn open(tx: Sender<Vec<u8>>, rx: UnboundedReceiver<Vec<u8>>) -> Result<Shell>; // syncs, sends `shell echo off`
    pub async fn exec(&self, cmd: &str) -> Result<String>;
    pub async fn exec_bg(&self, cmd: &str) -> Result<String>;  // background polling: not logged, yields to exec
    pub fn subscribe(&self) -> broadcast::Receiver<LogLine>;   // Sent / Received / Failed of every exec
    pub async fn stream(&self, start: &str, stop: &str) -> Result<ShellStream>; // drop sends stop
}
```

Inside: `Framer` (pure: bytes in → response out, split at the colored prompt, ANSI/CR stripped), one-at-a-time command queue, 10 s total timeout per command, resync via a unique unknown command (`__syncN` → `command not found`) at open and after a failed/cancelled command, `__ping` keep-alive for dongle (todo).

### eg-studio

```rust
impl Studio {
    pub async fn open(link: Link) -> Result<Studio>;
    pub async fn call(&self, req: studio::Request) -> Result<studio::Response>;
    pub fn notifications(&self) -> broadcast::Receiver<studio::Notification>; // lock state, etc.
}
```

Inside: prost codegen, frame escaping, request-id matching. Exposes prost types directly.

### eg-device

Domain layer; Marshmellow's logic lives here. No Dioxus.

```rust
impl Session {
    pub async fn connect(dev: &DeviceInfo) -> Result<Session>; // open + probe Capabilities
    pub fn caps(&self) -> watch::Receiver<Capabilities>;       // updated by background `board output` polling
    pub fn shell(&self) -> &Shell;
    pub fn studio(&self) -> Option<&Studio>;
    pub fn closed(&self) -> impl Future<Output = ()>;
}

pub struct Capabilities {
    pub kind: Kind,
    pub firmware: Firmware,        // version; sensor 3610 | 3395 (version >= 100 → 3395, major -= 100)
    pub output: Output,            // Usb | Ble | Esb
    pub shell_cmds: HashSet<String>,
    pub studio: Option<StudioState>, // locked: bool
}
```

Every feature module has the same shape:

```rust
// features/sensor.rs
pub struct SensorSettings { /* pointer_pct, twist_on, twist_reverse, twist_pct, sync_ms, plane_deg, sma_window */ }
pub fn parse(status: &str, pointer: &str) -> Result<SensorSettings>;  // pure
pub fn diff(old: &SensorSettings, new: &SensorSettings) -> Vec<String>; // pure
pub async fn read(s: &Shell) -> Result<SensorSettings>;
pub async fn apply(s: &Shell, saved: &SensorSettings, new: &SensorSettings) -> Result<SensorSettings>; // send edited fields, re-read
```

`features/keymap.rs` wraps Studio (get keymap / set binding / save changes) and the cross-protocol flow "save changes, then `keymap save N name` via shell".

### eg-firmware

Independent of device connections. Download release → verify sha256 → wait for UF2 drive → copy. Port of `../endgame-updater/update_endgame.py`.

### eg-app

```rust
// registry.rs
struct Feature {
    id: &'static str,
    title: &'static str,
    requires: fn(&Capabilities) -> Result<(), &'static str>, // Err = reason shown on disabled item
    page: Component,
}
```

- Dioxus signals are not `Send`; `eg-device` never sees them. One coroutine subscribes to `session.caps()` / `closed()` and writes signals. `Session` lives in context.
- Pages: `use_resource(|| sensor::read(shell))`, Apply → `spawn(sensor::apply(..))`.
- Sidebar is generated from the registry + capabilities.

### Data flow (Apply)

```
SensorForm ─spawn─► sensor::apply ─► Shell.exec("p2sm ...")×N ─► Link.tx ─► ble chunks
     ▲                                      ▲
     └──── device values ◄──── sensor::read ◄── Framer sees "endgame$ "
```

## Errors

One `thiserror` enum per crate. `eg-shell`: `Timeout(cmd)`, `Unsupported(cmd)`, `Closed`. `eg-device` adds `Parse(String)`. The app shows `Display` output.

## Tests

| Crate | What |
|---|---|
| eg-shell | Framer split edges; queue + timeout with an mpsc fake device |
| eg-studio | framing round-trip; request-id matching |
| eg-device | feature parse/diff against fixtures |
| eg-transport, eg-app | manual on hardware |

## Not doing

- No Channel trait: `Link` is just channels.
- No wrappers around Studio prost types.
- No protocol strings in `eg-app`.

## Open questions

1. USB: are shell and Studio on separate CDC ports, and how to tell them apart (interface number / description)?
2. BLE: do shell and Studio characteristics work concurrently on one connection?
3. Does `bluest` on Windows/macOS list a connected device that is not advertising the shell UUID? Fallback: `connected_devices_with_services`, or ask the user to trigger `&zbs_adv`.
4. Exact output of `p2sm status`, `p2sm sens pointer get`; is `p2sm … set` persisted across reboot?
5. Dioxus version: pin latest stable (0.8 is alpha as of 2026-11).
