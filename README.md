# Endgame App

Desktop configurator for the Endgame trackball over Bluetooth. See [ARCHITECTURE.md](ARCHITECTURE.md).

## Build

Requires Rust (https://rustup.rs).

```sh
cargo build --release -p eg-app       # app: target/release/endgame(.exe)
cargo test --workspace --exclude eg-app
```

- Windows: needs the WebView2 runtime (preinstalled on Windows 10/11).
- Linux: needs the webkit2gtk-4.1, gtk3, xdo, and dbus development packages,
  e.g. `apt install libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libdbus-1-dev pkg-config`.
