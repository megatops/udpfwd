// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Ding Zhaojie <zhaojie_ding@msn.com>

//! Safe wrappers for Windows Win32 API calls.
//!
//! Provides zero-cost abstractions over low-level Windows APIs:
//! - Window management: restore, foreground, style configuration
//! - Power notifications: register/unregister for resume events
//! - Message processing: wait and peek for power broadcast messages
//! - Tray icon restoration: detect Explorer restart after hibernate

use std::mem::MaybeUninit;

// -----------------------------------------------------------------------------
// Win32 Constants
// -----------------------------------------------------------------------------

const WS_MINIMIZEBOX: i32 = 0x0002_0000;
const WS_MAXIMIZEBOX: i32 = 0x0001_0000;
const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOZORDER: u32 = 0x0004;
const SW_RESTORE: i32 = 0x0009;
const GWL_STYLE: i32 = -16;

pub const PBT_APMRESUMEAUTOMATIC: u32 = 0x0007;
pub const PBT_APMRESUMESUSPEND: u32 = 0x0004;
const WM_POWERBROADCAST: u32 = 0x0218;
const PM_REMOVE: u32 = 0x0000;

const NIM_ADD: u32 = 0x0000;
const NIF_MESSAGE: u32 = 0x0001;
const NIF_ICON: u32 = 0x0002;
const NIF_TIP: u32 = 0x0004;

/// Callback message ID used by NWG for tray notifications.
const NWG_TRAY_MSG: u32 = 0x0400 + 102;

/// GUID for monitor power setting notifications.
const GUID_MONITOR_POWER_ON: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x02731015,
    data2: 0x4516,
    data3: 0x4993,
    data4: [0xA0, 0x98, 0xD4, 0xD2, 0x21, 0x3C, 0x1A, 0x2F],
};

// -----------------------------------------------------------------------------
// Window Management
// -----------------------------------------------------------------------------

/// Restores a minimized window and brings it to the foreground.
#[inline]
pub fn restore_and_foreground(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow};
    unsafe {
        ShowWindow(hwnd as *mut _, SW_RESTORE);
        SetForegroundWindow(hwnd as *mut _);
    }
}

/// Disables the maximize button and centers the window on the primary display.
#[inline]
pub fn configure_window_style_and_position(hwnd: isize) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, GetWindowLongW, GetWindowRect, SetWindowLongW, SetWindowPos,
        SM_CXSCREEN, SM_CYSCREEN,
    };
    unsafe {
        let style = GetWindowLongW(hwnd as *mut _, GWL_STYLE);
        SetWindowLongW(
            hwnd as *mut _,
            GWL_STYLE,
            style | WS_MINIMIZEBOX & !WS_MAXIMIZEBOX,
        );

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);

        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd as *mut _, &mut rect) != 0 {
            let win_w = rect.right - rect.left;
            let win_h = rect.bottom - rect.top;
            SetWindowPos(
                hwnd as *mut _,
                std::ptr::null_mut(),
                (screen_w - win_w) / 2,
                (screen_h - win_h) / 2,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
        }
    }
}

// -----------------------------------------------------------------------------
// Power Notifications
// -----------------------------------------------------------------------------

/// Registers for power setting change notifications.
///
/// Returns a notification handle, or 0 on failure.
#[inline]
pub fn register_power_notification(hwnd: isize) -> isize {
    use windows_sys::Win32::System::Power::RegisterPowerSettingNotification;
    unsafe {
        RegisterPowerSettingNotification(hwnd as *mut std::ffi::c_void, &GUID_MONITOR_POWER_ON, 0)
    }
}

/// Unregisters power setting notifications.
#[inline]
pub fn unregister_power_notification(handle: isize) {
    use windows_sys::Win32::System::Power::UnregisterPowerSettingNotification;
    unsafe {
        let _ = UnregisterPowerSettingNotification(handle);
    }
}

// -----------------------------------------------------------------------------
// Message Processing
// -----------------------------------------------------------------------------

/// Waits for Windows messages with a 500ms timeout.
///
/// Returns `false` on error, `true` otherwise.
#[inline]
pub fn wait_for_messages() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::MsgWaitForMultipleObjects;
    unsafe { MsgWaitForMultipleObjects(0, std::ptr::null_mut(), 0, 500, 0x01) != 0xFFFFFFFF }
}

/// Peeks at and removes a pending `WM_POWERBROADCAST` message.
///
/// Returns `true` if a message was retrieved.
#[inline]
pub fn peek_power_message(
    msg: &mut MaybeUninit<windows_sys::Win32::UI::WindowsAndMessaging::MSG>,
) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::PeekMessageW;
    unsafe {
        PeekMessageW(
            msg.as_mut_ptr(),
            std::ptr::null_mut(),
            WM_POWERBROADCAST,
            WM_POWERBROADCAST,
            PM_REMOVE,
        ) != 0
    }
}

// -----------------------------------------------------------------------------
// Tray Icon Restoration
// -----------------------------------------------------------------------------

/// Registers the `TaskbarCreated` message to detect Explorer restarts.
///
/// When Explorer.exe restarts (e.g., after hibernate), all tray icons are
/// destroyed. Windows broadcasts this registered message so applications
/// can re-add their icons.
///
/// Returns the message ID, or 0 on failure.
pub fn register_taskbar_created_message() -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW;
    unsafe {
        let wide: Vec<u16> = "TaskbarCreated\0".encode_utf16().collect();
        RegisterWindowMessageW(wide.as_ptr())
    }
}

/// Re-adds a tray icon after Explorer restarts using `Shell_NotifyIconW(NIM_ADD)`.
///
/// NWG's `set_icon()` uses `NIM_MODIFY`, which silently fails when the icon
/// has been removed by Explorer. This function rebuilds the `NOTIFYICONDATAW`
/// and calls `NIM_ADD` to re-register the icon from scratch.
pub fn readd_tray_icon(hwnd: isize, icon_handle: isize, tip: &[u16]) -> bool {
    use windows_sys::Win32::UI::Shell::{Shell_NotifyIconW, NOTIFYICONDATAW};
    unsafe {
        let mut nid = std::mem::zeroed::<NOTIFYICONDATAW>();
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.hWnd = hwnd as *mut _;
        nid.uCallbackMessage = NWG_TRAY_MSG;
        nid.hIcon = icon_handle as *mut _;
        let tip_len = tip.len().min(128).saturating_sub(1);
        std::ptr::copy_nonoverlapping(tip.as_ptr(), nid.szTip.as_mut_ptr(), tip_len);
        Shell_NotifyIconW(NIM_ADD, &nid) != 0
    }
}