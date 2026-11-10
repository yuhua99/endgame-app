# Endgame App — Architecture

Desktop app for configuring the efog.tech Endgame trackball (ZMK firmware) over BLE.
Goal: cover all Marshmellow UI features plus ZMK-Studio-style key remapping.

Stack: Rust only. UI uses Dioxus desktop (native Rust process rendering to a webview via wry). No Tauri, no IPC, no TypeScript.

## Device protocols

The device exposes up to three channels; which ones are available depends on how it is connected. The app connects over BLE and uses the shell and studio channels.

| Connection | shell (text) | studio (protobuf RPC) | data (stream) |
|---|---|---|---|
| BLE direct | GATT svc `c901c4e9-5770-4bf1-96b2-2dd287813e6e`, chrc `c901c4ea-…` (write-without-resp + notify) | GATT svc `00000000-0196-6107-c967-c5cfb1c2482a`, chrc `00000001-0196-6107-c967-c5cfb1c2482a` | chrc `c901c4eb-…` (notify) |
| USB direct (VID 0x0011, PID 6) | CDC ACM serial | CDC ACM serial (unverified) | same as shell |
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
          /         \
     eg-device      dioxus
    /    |     \
eg-shell eg-studio eg-transport
    \      |      /
  tokio::mpsc (Link = a pair of channels)
```

Only `eg-app` depends on Dioxus. `eg-shell` / `eg-studio` do not depend on `eg-transport`; they take a `Link`'s channels, so tests feed them plain channels. `eg-app` also uses `eg-transport` directly, to scan for devices.

```
endgame-app/
  Cargo.toml                 [workspace]
  crates/
    eg-transport/  src/{lib, ble}.rs
    eg-shell/      src/{lib, framer, client}.rs
    eg-studio/     src/{lib, framing, client}.rs, build.rs (prost), proto/ (zmk-studio-messages)
    eg-device/     src/{lib, session}.rs
                   src/{sensor, rtcfg, profiles, lighting, board, keymap}.rs   features
                   tests/fixtures/v1.6.2/*.txt                       recorded device output
    eg-app/        src/{main, connect, device}.rs                    app, connect page, device page
                   src/{sensor, advanced, keymap, profiles, lighting, logs}.rs   tabs
                   src/{editable, widgets, endgame, hid}.rs          form state, form rows, trackball drawing, HID usages
                   assets/{style.css, hid_usages.tsv, behavior_short_names.tsv}   tables from zmk-studio, keep NOTICE
```

### eg-transport

The only crate that touches the OS / hardware.

```rust
pub struct Link { pub tx: mpsc::Sender<Vec<u8>>, pub rx: mpsc::UnboundedReceiver<Vec<u8>> } // rx ends = channel closed
pub struct DeviceInfo { pub name: String, /* bluest adapter and device */ }
pub async fn scan(found: mpsc::UnboundedSender<DeviceInfo>) -> Result<()>; // sends devices as found: connected first, then advertising
pub async fn open(dev: &DeviceInfo) -> Result<Connection>;
impl Connection {
    pub async fn shell(&self) -> Result<Link>;          // subscribes before returning
    pub async fn studio(&self) -> Option<Result<Link>>; // None = the firmware has no Studio service
}
```

- BLE via `bluest` (same as zmk-studio). BLE work runs on a tokio worker thread: the UI thread is a COM single-threaded apartment (WebView2), where awaiting WinRT Bluetooth calls can hang.
- `scan` asks each connected device for the shell service directly, because Windows matches against a cached service list that may lack it and ZMK does not advertise it.
- One connection serves both links. Each link is a task that owns its characteristic and reads and writes concurrently; it ends when the device disconnects, a read or write fails, or the caller drops either channel end.
- Hides BLE chunking (96 B / 20 ms).

### eg-shell

```rust
impl Shell {                                                   // Clone (Arc inside)
    pub async fn open(tx: Sender<Vec<u8>>, rx: UnboundedReceiver<Vec<u8>>) -> Result<Shell>; // syncs, sends `shell echo off`
    pub async fn exec(&self, cmd: &str) -> Result<String>;
    pub async fn exec_bg(&self, cmd: &str) -> Result<String>;  // for polling: not logged
    pub fn subscribe(&self) -> broadcast::Receiver<LogLine>;   // Sent / Received / Failed of every exec
}
```

Inside: `Framer` (pure: bytes in → response out, split at the colored prompt, ANSI/CR stripped), firmware log lines removed from responses, one-at-a-time command queue, 10 s total timeout per command, resync via a unique unknown command (`__syncN` → `command not found`) at open and after a failed/cancelled command.

### eg-studio

```rust
impl Studio {                                                  // Clone (Arc inside)
    pub fn open(tx: Sender<Vec<u8>>, rx: UnboundedReceiver<Vec<u8>>) -> Studio; // spawns the reader task
    pub async fn call(&self, req: studio::request::Subsystem) -> Result<studio::request_response::Subsystem>;
    pub fn notifications(&self) -> broadcast::Receiver<studio::Notification>;   // lock state, unsaved changes
}
```

Inside: prost codegen (protox in build.rs), frame escaping, request-id matching, one call at a time with a 5 s timeout. Exposes prost types directly.

### eg-device

Domain layer; Marshmellow's logic lives here. No Dioxus.

```rust
pub struct Session { pub name: String, pub firmware: String, /* shell, studio, connection */ } // Clone
impl Session {
    pub async fn connect(dev: &DeviceInfo) -> Result<Session>; // open, shell, `board version`, Studio if present
    pub fn shell(&self) -> &Shell;
    pub fn studio(&self) -> Option<&Studio>;
}
```

The connection closes when the last clone drops.

| Module | Covers | Over |
|---|---|---|
| `sensor` | sensitivity, twist, SMA, sync window, rotation, frame sync, sensitivity behaviors; surface quality | shell: `p2sm`, `rrl`, `plane`, `rtcfg`, `sensor surface` |
| `rtcfg` | runtime parameters as a generic list | shell: `rtcfg` |
| `profiles` | keymap slots, output assignments, autoswitch, OS mode | shell: `keymap`, `bistable`, `rtcfg`; asks Studio about unsaved edits |
| `lighting` | light and vibration per event, light switch, brightness, RGB support and override | shell: `argb`, `board rgb`, `board layers`, `rtcfg` |
| `board` | battery, active output | shell: `board status`, `board output` |
| `keymap` | layout, layers, behaviors, bindings, save/discard, lock and unsaved-changes events | Studio |

Settings modules have the same shape:

```rust
// sensor.rs
pub struct SensorSettings { /* pointer_pct, twist_on, twist_reverse, twist_pct, sync_ms, plane_deg, sma_window, ... */ }
pub fn parse(status: &str, pointer: &str, rrl: &str, plane: Option<&str>) -> Result<SensorSettings>; // pure
pub fn diff(old: &SensorSettings, new: &SensorSettings) -> Vec<String>; // pure: commands for the edited fields
pub async fn read(shell: &Shell) -> Result<SensorSettings>;
pub async fn apply(shell: &Shell, saved: &SensorSettings, new: &SensorSettings) -> Result<SensorSettings>; // send the diff, re-read
```

`profiles::change` crosses protocols: a change that stores or replaces the keymap is refused while Studio reports unsaved edits.

### eg-app

- `App` holds a `Signal<Option<Session>>`: `None` shows the connect page, `Some` the device page (top bar and tabs). Pages and tabs get the signal as a prop.
- Dioxus signals are not `Send`; `eg-device` never sees them.
- A tab is mounted on first visit and then only hidden, so switching tabs never cancels a running operation.
- Sensor, Advanced, and Lighting keep an `Editable<T>`: `saved` mirrors the device, `form` holds the edits, Apply sends the difference. They read the device again when reopened without edits.
- Polling lives here: surface quality every 100 ms while the Sensor tab is visible, battery and active output every 5 s. Both use `exec_bg`.
- The 5 s poll is also the heartbeat: when it fails (link closed, or no answer within the shell timeout), the app drops the session and shows the connect page with the reason.
- The device page subscribes to the shell log when it mounts; the Logs tab shows it and sends typed commands.

### Data flow (Apply)

```
SensorTab ─spawn─► sensor::apply ─► Shell.exec("p2sm ...")×N ─► Link.tx ─► ble chunks
    ▲                                      ▲
    └──── device values ◄──── sensor::read ◄── Framer sees "endgame$ "
```

## Errors

One `thiserror` enum per crate. `eg-shell`: `Timeout(cmd)`, `Unsupported(cmd)`, `Closed`. `eg-device` wraps the transport, shell, and Studio errors and adds `Rejected`, `UnsavedKeymap`, and `Parse { cmd, output }`. The app shows `Display` output.

## Tests

| Crate | What |
|---|---|
| eg-shell | Framer split edges; queue, timeout, resync, and log with an mpsc fake device |
| eg-studio | framing round-trip; request-id matching |
| eg-device | parse/diff against fixtures recorded over BLE |
| eg-app | pure helpers only; the UI is tested by hand on hardware |
| eg-transport | by hand on hardware |

## Not doing

- No Channel trait: `Link` is just channels.
- No wrappers around Studio prost types.
- No protocol strings in `eg-app`.
