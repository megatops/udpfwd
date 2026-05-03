// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Ding Zhaojie <zhaojie_ding@msn.com>

//! Safe wrappers for Windows Win32 API calls.
//!
//! Provides zero-cost abstractions over low-level Windows APIs:
//! - Window management: restore, foreground, style configuration
//! - Power notifications: register/unregister for resume events
//! - Message processing: wait and peek for power broadcast messages

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
        GetSystemMetrics, GetWindowLongW, GetWindowRect, SetWindowLongW, SetWindowPos, SM_CXSCREEN,
        SM_CYSCREEN,
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
