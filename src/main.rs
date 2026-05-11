// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Ding Zhaojie <zhaojie_ding@msn.com>

#![windows_subsystem = "windows"]

//! Native Windows GUI application for forwarding UDP packets.
//!
//! Provides a minimal window with input fields for local port, target IP, and
//! target port. A Start/Stop button controls forwarding. The application
//! minimizes to the system tray and displays real-time PPS in the status bar.
//!
//! ## Features
//!
//! - IPv4-only forwarding for maximum throughput (>8000 pps)
//! - System tray with Show/Exit context menu
//! - Tray icon restoration after Explorer restart (TaskbarCreated)
//! - Power-resume recovery (auto-restart after sleep/hibernate)
//! - Embedded application icon
//! - Registry-based configuration persistence
//! - CLI support for scripted startup (`-l`, `-t`, `-a`)

mod forwarder;
mod win32;

use clap::Parser;
use native_windows_derive as nwd;
use native_windows_gui as nwg;
use nwd::NwgUi;
use nwg::NativeUi;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use forwarder::{resolve_target, Config, UdpForwarder};
use win32::*;

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

const ICON_RESOURCE_ID: usize = 1;
const FONT_FAMILY: &str = "Segoe UI";
const FONT_SIZE: u32 = 18;
const WINDOW_WIDTH: i32 = 284;
const WINDOW_HEIGHT: i32 = 148;
const TIMER_INTERVAL_MS: u32 = 1000;
const TRAY_TIP: &str = "UDP Forwarder";

// -----------------------------------------------------------------------------
// CLI Arguments
// -----------------------------------------------------------------------------

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Local UDP port to listen on.
    #[arg(short, long)]
    local_port: Option<u16>,
    /// Target address as `IP:PORT`.
    #[arg(short, long)]
    target: Option<String>,
    /// Auto-start forwarding on launch.
    #[arg(short, long, action)]
    auto_start: bool,
}

/// Parses `IP:PORT` format, returning `(host, port)`.
fn parse_target(s: &str) -> Option<(String, u16)> {
    let idx = s.rfind(':')?;
    let host = s[..idx].to_string();
    let port = s[idx + 1..].parse().ok()?;
    Some((host, port))
}

// -----------------------------------------------------------------------------
// Application State
// -----------------------------------------------------------------------------

/// Shared state between GUI and forwarding threads.
struct AppState {
    forwarder: Mutex<UdpForwarder>,
    is_running: AtomicBool,
    last_count: AtomicU64,
}

static APP_STATE: OnceLock<Arc<AppState>> = OnceLock::new();

/// Returns the global application state, initializing it on first access.
fn get_state() -> Arc<AppState> {
    APP_STATE
        .get_or_init(|| {
            Arc::new(AppState {
                forwarder: Mutex::new(UdpForwarder::new()),
                is_running: AtomicBool::new(false),
                last_count: AtomicU64::new(0),
            })
        })
        .clone()
}

// -----------------------------------------------------------------------------
// UI Definition
// -----------------------------------------------------------------------------

#[derive(Default, NwgUi)]
pub struct App {
    #[nwg_control(size: (WINDOW_WIDTH, WINDOW_HEIGHT), title: "UDP Forwarder", flags: "WINDOW|VISIBLE")]
    #[nwg_events(OnWindowClose: [App::on_close], OnWindowMinimize: [App::on_minimize])]
    window: nwg::Window,

    #[nwg_control(parent: window, popup: true)]
    tray_menu: nwg::Menu,

    #[nwg_control(parent: tray_menu, text: "Show")]
    #[nwg_events(OnMenuItemSelected: [App::on_tray_show])]
    menu_show: nwg::MenuItem,

    #[nwg_control(parent: tray_menu, text: "Exit")]
    #[nwg_events(OnMenuItemSelected: [App::on_tray_exit])]
    menu_exit: nwg::MenuItem,

    #[nwg_control(icon: Some(&nwg::Icon::default()), tip: Some(TRAY_TIP))]
    #[nwg_events(OnContextMenu: [App::on_tray_menu], MousePressLeftUp: [App::on_tray_activate])]
    tray: nwg::TrayNotification,

    #[nwg_layout(parent: window, spacing: 2, margin: [1, 1, 1, 1])]
    grid: nwg::GridLayout,

    #[nwg_control(text: "Local Port:")]
    #[nwg_layout_item(layout: grid, row: 0, col: 0)]
    lbl_local: nwg::Label,

    #[nwg_control(text: "")]
    #[nwg_layout_item(layout: grid, row: 0, col: 1)]
    inp_local: nwg::TextInput,

    #[nwg_control(text: "Target IP:")]
    #[nwg_layout_item(layout: grid, row: 1, col: 0)]
    lbl_ip: nwg::Label,

    #[nwg_control(text: "")]
    #[nwg_layout_item(layout: grid, row: 1, col: 1)]
    inp_ip: nwg::TextInput,

    #[nwg_control(text: "Target Port:")]
    #[nwg_layout_item(layout: grid, row: 2, col: 0)]
    lbl_target: nwg::Label,

    #[nwg_control(text: "")]
    #[nwg_layout_item(layout: grid, row: 2, col: 1)]
    inp_target: nwg::TextInput,

    #[nwg_control(text: "Start")]
    #[nwg_layout_item(layout: grid, row: 3, col: 0, col_span: 2)]
    #[nwg_events(OnButtonClick: [App::on_start])]
    btn_start: nwg::Button,

    #[nwg_control(text: "Ready")]
    #[nwg_layout_item(layout: grid, row: 4, col: 0, col_span: 2)]
    status_bar: nwg::StatusBar,

    #[nwg_control(parent: window, interval: TIMER_INTERVAL_MS)]
    #[nwg_events(OnTimerTick: [App::on_timer])]
    #[allow(deprecated)]
    timer: nwg::Timer,
}

impl App {
    // -- Window events --

    fn on_close(&self) {
        self.timer.stop();
        let state = get_state();
        state.is_running.store(false, Ordering::SeqCst);
        state.forwarder.lock().unwrap().stop();
        nwg::stop_thread_dispatch();
    }

    fn on_minimize(&self) {
        self.window.set_visible(false);
    }

    // -- Tray events --

    fn on_tray_menu(&self) {
        let (x, y) = nwg::GlobalCursor::position();
        self.tray_menu.popup(x, y);
    }

    fn on_tray_activate(&self) {
        if self.window.visible() {
            self.window.set_visible(false);
        } else {
            self.show_and_activate();
        }
    }

    fn on_tray_show(&self) {
        self.show_and_activate();
    }

    fn on_tray_exit(&self) {
        self.on_close();
    }

    fn show_and_activate(&self) {
        if let Some(hwnd) = self.window.handle.hwnd() {
            restore_and_foreground(hwnd as isize);
        }
        self.window.set_visible(true);
        refresh_icon(&self.window, &self.tray);
    }

    // -- Timer --

    fn on_timer(&self) {
        let state = get_state();
        if !state.is_running.load(Ordering::SeqCst) {
            return;
        }
        let count = state.forwarder.lock().unwrap().packet_count();
        let last = state.last_count.load(Ordering::SeqCst);
        let pps = count - last;
        state.last_count.store(count, Ordering::SeqCst);
        self.status_bar
            .set_text(0, &format!("Forwarding: total {count} @ {pps} pps"));
    }

    // -- Start/Stop --

    fn on_start(&self) {
        let state = get_state();
        if state.is_running.load(Ordering::SeqCst) {
            self.stop_forwarding(&state);
        } else {
            self.start_forwarding(&state);
        }
    }

    fn stop_forwarding(&self, state: &AppState) {
        self.timer.stop();
        state.is_running.store(false, Ordering::SeqCst);
        state.forwarder.lock().unwrap().stop();
        state.last_count.store(0, Ordering::SeqCst);
        self.status_bar.set_text(0, "Stopped");
        self.btn_start.set_text("Start");
        stop_power_listener();
    }

    fn start_forwarding(&self, state: &AppState) {
        let local: i32 = self.inp_local.text().parse().unwrap_or(0);
        let ip = self.inp_ip.text();
        let target: i32 = self.inp_target.text().parse().unwrap_or(0);

        if !(1..=65535).contains(&local) || !(1..=65535).contains(&target) || ip.is_empty() {
            self.status_bar.set_text(0, "Error: Invalid port or IP");
            return;
        }

        let local_port = local as u16;
        let target_port = target as u16;

        let target_addr = match resolve_target(&ip, target_port) {
            Ok(addr) => addr,
            Err(e) => {
                self.status_bar.set_text(0, &format!("Error: {e}"));
                return;
            }
        };

        let mut fwd = state.forwarder.lock().unwrap();
        match fwd.start(local_port, target_addr) {
            Ok(()) => {
                let _ = Config {
                    local_port,
                    target_ip: ip.clone(),
                    target_port,
                }
                .save();
                state.is_running.store(true, Ordering::SeqCst);
                state.last_count.store(0, Ordering::SeqCst);
                self.status_bar.set_text(0, "Forwarding: total 0 @ 0 pps");
                self.btn_start.set_text("Stop");
                self.timer.start();
                start_power_listener();
            }
            Err(e) => self.status_bar.set_text(0, &format!("Error: {e}")),
        }
    }
}

// -----------------------------------------------------------------------------
// Icon Helpers
// -----------------------------------------------------------------------------

/// Loads the embedded application icon from the executable resources.
fn load_embedded_icon() -> Option<nwg::Icon> {
    let embed = nwg::EmbedResource::load(None).ok()?;
    nwg::Icon::from_embed(&embed, Some(ICON_RESOURCE_ID), None).ok()
}

/// Applies the embedded icon to the window title bar and system tray.
///
/// Uses `Shell_NotifyIconW(NIM_ADD)` via `tray.set_icon()` to re-register the
/// tray icon, which is idempotent and works after Explorer restarts (unlike
/// `NIM_MODIFY` which silently fails when the icon has been removed).
fn refresh_icon(window: &nwg::Window, tray: &nwg::TrayNotification) {
    if let Some(icon) = load_embedded_icon() {
        window.set_icon(Some(&icon));
        tray.set_icon(&icon);
    }
}

// -----------------------------------------------------------------------------
// Font Helpers
// -----------------------------------------------------------------------------

/// Applies the given font to all UI controls.
fn apply_font(app: &App, font: &nwg::Font) {
    app.lbl_local.set_font(Some(font));
    app.inp_local.set_font(Some(font));
    app.lbl_ip.set_font(Some(font));
    app.inp_ip.set_font(Some(font));
    app.lbl_target.set_font(Some(font));
    app.inp_target.set_font(Some(font));
    app.btn_start.set_font(Some(font));
    app.status_bar.set_font(Some(font));
}

// -----------------------------------------------------------------------------
// Power Event Listener
// -----------------------------------------------------------------------------

static POWER_HWND: OnceLock<isize> = OnceLock::new();
static POWER_LISTENING: AtomicBool = AtomicBool::new(false);

/// Starts the power-resume listener thread.
///
/// Registers for monitor power events and auto-restarts the forwarder when the
/// system resumes from sleep or hibernation.
fn start_power_listener() {
    if POWER_LISTENING.swap(true, Ordering::SeqCst) {
        return;
    }

    let hwnd = match POWER_HWND.get() {
        Some(&h) => h,
        None => {
            POWER_LISTENING.store(false, Ordering::SeqCst);
            return;
        }
    };

    thread::spawn(move || {
        let handle = register_power_notification(hwnd);
        if handle == 0 {
            POWER_LISTENING.store(false, Ordering::SeqCst);
            return;
        }

        loop {
            if !POWER_LISTENING.load(Ordering::SeqCst) {
                break;
            }

            if !wait_for_messages() {
                continue;
            }

            let mut msg = std::mem::MaybeUninit::<
                windows_sys::Win32::UI::WindowsAndMessaging::MSG,
            >::uninit();
            while peek_power_message(&mut msg) {
                let msg = unsafe { msg.assume_init() };
                if msg.wParam == PBT_APMRESUMEAUTOMATIC as usize
                    || msg.wParam == PBT_APMRESUMESUSPEND as usize
                {
                    let state = get_state();
                    if state.is_running.load(Ordering::SeqCst) {
                        let mut fwd = state.forwarder.lock().unwrap();
                        if let Err(e) = fwd.restart() {
                            eprintln!("Failed to restart forwarder: {e}");
                        }
                    }
                }
            }
        }

        unregister_power_notification(handle);
        POWER_LISTENING.store(false, Ordering::SeqCst);
    });
}

/// Signals the power-resume listener thread to stop.
fn stop_power_listener() {
    POWER_LISTENING.store(false, Ordering::SeqCst);
}

// -----------------------------------------------------------------------------
// Entry Point
// -----------------------------------------------------------------------------

fn main() {
    let args = Args::parse();

    nwg::init().expect("Failed to init Native Windows GUI");
    let app = App::build_ui(Default::default()).expect("Failed to build UI");

    // Apply embedded icon and font
    refresh_icon(&app.window, &app.tray);
    let mut font = nwg::Font::default();
    nwg::Font::builder()
        .family(FONT_FAMILY)
        .size(FONT_SIZE)
        .build(&mut font)
        .ok();
    apply_font(&app, &font);

    // Populate UI from saved config; CLI arguments override if provided
    let mut cfg = Config::load();
    if let Some(port) = args.local_port {
        cfg.local_port = port;
    }
    if let Some((ip, port)) = args.target.as_deref().and_then(parse_target) {
        cfg.target_ip = ip;
        cfg.target_port = port;
    }
    app.inp_local.set_text(&cfg.local_port.to_string());
    app.inp_ip.set_text(&cfg.target_ip);
    app.inp_target.set_text(&cfg.target_port.to_string());

    // Configure window style, center on screen, and store HWND for power listener
    let hwnd = app.window.handle.hwnd().expect("Failed to get HWND");
    let hwnd_val = hwnd as isize;
    POWER_HWND.set(hwnd_val).ok();
    configure_window_style_and_position(hwnd_val);

    // Listen for TaskbarCreated to restore tray icon after Explorer restart
    let taskbar_msg = register_taskbar_created_message();
    if taskbar_msg != 0 {
        let tray_hwnd = hwnd_val;
        nwg::bind_raw_event_handler(&app.window.handle, 0x10001, move |_h, msg, _w, _l| {
            if msg == taskbar_msg
                && let Some(icon) = load_embedded_icon()
            {
                let tip: Vec<u16> = format!("{TRAY_TIP}\0").encode_utf16().collect();
                readd_tray_icon(tray_hwnd, icon.handle as isize, &tip);
            }
            None
        })
        .ok();
    }

    if args.auto_start {
        app.on_start();
    }

    nwg::dispatch_thread_events();
}
