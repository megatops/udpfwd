// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Ding Zhaojie <zhaojie_ding@msn.com>

#![windows_subsystem = "windows"]

mod forwarder;

use clap::Parser;
use native_windows_derive as nwd;
use native_windows_gui as nwg;
use nwd::NwgUi;
use nwg::NativeUi;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use forwarder::{Config, UdpForwarder};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, num_args = 1..)]
    local_port: Option<u16>,
    #[arg(short = 'i', long, num_args = 1..)]
    target_ip: Option<String>,
    #[arg(short, long, num_args = 1..)]
    target_port: Option<u16>,
    #[arg(short, long, action)]
    auto_start: bool,
}

pub struct AppState {
    pub forwarder: Mutex<UdpForwarder>,
    pub is_running: AtomicBool,
    pub last_count: AtomicU64,
}

static APP_STATE: OnceLock<Arc<AppState>> = OnceLock::new();

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

#[derive(Default, NwgUi)]
pub struct App {
    #[nwg_control(size: (284, 148), title: "UDP Forwarder", flags: "WINDOW|VISIBLE")]
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

    #[nwg_control(icon: Some(&nwg::Icon::default()), tip: Some("UDP Forwarder"))]
    #[nwg_events(OnContextMenu: [App::on_tray_menu], MousePressLeftUp: [App::on_tray_activate])]
    tray: nwg::TrayNotification,

    #[nwg_layout(parent: window, spacing: 2, margin: [1, 1, 1, 1])]
    grid: nwg::GridLayout,

    #[nwg_control(text: "Local Port:")]
    #[nwg_layout_item(layout: grid, row: 0, col: 0)]
    lbl_local: nwg::Label,

    #[nwg_control(text: "8888")]
    #[nwg_layout_item(layout: grid, row: 0, col: 1)]
    inp_local: nwg::TextInput,

    #[nwg_control(text: "Target IP:")]
    #[nwg_layout_item(layout: grid, row: 1, col: 0)]
    lbl_ip: nwg::Label,

    #[nwg_control(text: "192.168.0.1")]
    #[nwg_layout_item(layout: grid, row: 1, col: 1)]
    inp_ip: nwg::TextInput,

    #[nwg_control(text: "Target Port:")]
    #[nwg_layout_item(layout: grid, row: 2, col: 0)]
    lbl_target: nwg::Label,

    #[nwg_control(text: "8888")]
    #[nwg_layout_item(layout: grid, row: 2, col: 1)]
    inp_target: nwg::TextInput,

    #[nwg_control(text: "Start")]
    #[nwg_layout_item(layout: grid, row: 3, col: 0, col_span: 2)]
    #[nwg_events(OnButtonClick: [App::on_start])]
    btn_start: nwg::Button,

    #[nwg_control(text: "Ready")]
    #[nwg_layout_item(layout: grid, row: 4, col: 0, col_span: 2)]
    status_bar: nwg::StatusBar,

    #[nwg_control(parent: window, interval: 1000)]
    #[nwg_events(OnTimerTick: [App::on_timer])]
    #[allow(deprecated)]
    timer: nwg::Timer,
}

impl App {
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

    fn show_and_activate(&self) {
        let Some(hwnd) = self.window.handle.hwnd() else {
            return;
        };

        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetForegroundWindow, ShowWindow, SW_RESTORE,
        };
        unsafe {
            ShowWindow(hwnd as *mut _, SW_RESTORE);
            SetForegroundWindow(hwnd as *mut _);
        }
        self.window.set_visible(true);
    }

    fn on_tray_exit(&self) {
        self.on_close();
    }

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

    fn on_start(&self) {
        let state = get_state();

        if state.is_running.load(Ordering::SeqCst) {
            self.timer.stop();
            state.is_running.store(false, Ordering::SeqCst);
            state.forwarder.lock().unwrap().stop();
            state.last_count.store(0, Ordering::SeqCst);
            self.status_bar.set_text(0, "Stopped");
            self.btn_start.set_text("Start");
            return;
        }

        let local: i32 = self.inp_local.text().parse().unwrap_or(0);
        let ip = self.inp_ip.text();
        let target: i32 = self.inp_target.text().parse().unwrap_or(0);

        if !(1..=65535).contains(&local) || !(1..=65535).contains(&target) || ip.is_empty() {
            self.status_bar.set_text(0, "Error: Invalid port or IP");
            return;
        }

        let local_port = local as u16;
        let target_port = target as u16;

        let mut fwd = state.forwarder.lock().unwrap();
        match fwd.start(local_port, &ip, target_port) {
            Ok(()) => {
                let cfg = Config {
                    local_port,
                    target_ip: ip.clone(),
                    target_port,
                };
                let _ = cfg.save();
                state.is_running.store(true, Ordering::SeqCst);
                state.last_count.store(0, Ordering::SeqCst);
                self.status_bar.set_text(0, "Forwarding: total 0 @ 0 pps");
                self.btn_start.set_text("Stop");
                self.timer.start();
            }
            Err(e) => self.status_bar.set_text(0, &format!("Error: {e}")),
        }
    }
}

fn main() {
    let args = Args::parse();

    nwg::init().expect("Failed to init Native Windows GUI");

    let app = App::build_ui(Default::default()).expect("Failed to build UI");

    let embed = nwg::EmbedResource::load(None).expect("Failed to load embed");
    let icon = nwg::Icon::from_embed(&embed, Some(1), None).ok();
    if let Some(ref icon) = icon {
        app.window.set_icon(Some(icon));
        app.tray.set_icon(icon);
    }

    let mut font = nwg::Font::default();
    nwg::Font::builder()
        .family("Segoe UI")
        .size(18)
        .build(&mut font)
        .ok();

    let font = &font;
    app.lbl_local.set_font(Some(font));
    app.inp_local.set_font(Some(font));
    app.lbl_ip.set_font(Some(font));
    app.inp_ip.set_font(Some(font));
    app.lbl_target.set_font(Some(font));
    app.inp_target.set_font(Some(font));
    app.btn_start.set_font(Some(font));
    app.status_bar.set_font(Some(font));

    let cfg = Config::load();

    app.inp_local
        .set_text(&args.local_port.unwrap_or(cfg.local_port).to_string());
    app.inp_ip
        .set_text(args.target_ip.as_deref().unwrap_or(&cfg.target_ip));
    app.inp_target
        .set_text(&args.target_port.unwrap_or(cfg.target_port).to_string());

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, GetWindowLongW, SetWindowLongW, SetWindowPos, GWL_STYLE, SM_CXSCREEN,
        SM_CYSCREEN,
    };

    let hwnd = app.window.handle.hwnd().expect("Failed to get HWND");
    let hwnd = hwnd as *mut std::ffi::c_void;

    unsafe {
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        SetWindowLongW(hwnd, GWL_STYLE, style | 0x00020000 & !0x00010000);
        let (w, h) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        SetWindowPos(hwnd, std::ptr::null_mut(), w / 2, h / 2, 0, 0, 0x0001);
    }

    if args.auto_start {
        app.on_start();
    }

    nwg::dispatch_thread_events();
}
