//! Endgame trackball configurator (Dioxus desktop).
//!
//! Screens: connect (scan + pick a BLE device) -> device (tabs; only Sensor is implemented).

mod advanced;
mod connect;
mod device;
mod editable;
mod endgame;
mod hid;
mod keymap;
mod lighting;
mod logs;
mod profiles;
mod sensor;
mod widgets;

use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use dioxus::prelude::*;
use eg_device::Session;

const CSS: &str = include_str!("../assets/style.css");

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let window = WindowBuilder::new().with_title("Endgame").with_inner_size(LogicalSize::new(1280.0, 720.0));
    dioxus::LaunchBuilder::desktop().with_cfg(Config::new().with_window(window).with_menu(None)).launch(App);
}

#[component]
fn App() -> Element {
    // Some = connected. Pages set it to connect or disconnect.
    let session = use_signal(|| None::<Session>);
    // The connect page's status line. The device page writes it when the connection is lost.
    let status = use_signal(String::new);
    rsx! {
        style { "{CSS}" }
        if session.read().is_some() {
            device::DevicePage { session, connect_status: status }
        } else {
            connect::ConnectPage { session, status }
        }
    }
}
