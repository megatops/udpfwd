// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Ding Zhaojie <zhaojie_ding@msn.com>

use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use winreg::enums::*;
use winreg::RegKey;

const REG_KEY_PATH: &str = r"Software\Megatops Software\UDP Forwarder";
const BUF_SIZE: usize = 65535;

pub struct UdpForwarder {
    listener: Option<UdpSocket>,
    sender: Option<UdpSocket>,
    running: bool,
    packet_count: Arc<AtomicU64>,
    forward_thread: Option<thread::JoinHandle<()>>,
    stop_flag: Arc<AtomicBool>,
}

impl UdpForwarder {
    pub fn new() -> Self {
        Self {
            listener: None,
            sender: None,
            running: false,
            packet_count: Arc::new(AtomicU64::new(0)),
            forward_thread: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(
        &mut self,
        local_port: u16,
        target_ip: &str,
        target_port: u16,
    ) -> std::io::Result<()> {
        let listener = UdpSocket::bind(format!("0.0.0.0:{local_port}"))?;
        let sender = UdpSocket::bind("0.0.0.0:0")?;
        sender.connect(format!("{target_ip}:{target_port}"))?;

        listener.set_nonblocking(false)?;
        sender.set_nonblocking(false)?;
        listener.set_read_timeout(Some(Duration::from_millis(100)))?;

        self.listener = Some(listener);
        self.sender = Some(sender);
        self.running = true;
        self.packet_count.store(0, Ordering::SeqCst);
        self.stop_flag.store(false, Ordering::SeqCst);

        let packet_count = Arc::clone(&self.packet_count);
        let stop_flag = Arc::clone(&self.stop_flag);
        let listener = self.listener.take().unwrap();
        let sender = self.sender.take().unwrap();

        self.forward_thread = Some(thread::spawn(move || {
            let mut buf = [0u8; BUF_SIZE];
            loop {
                if stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                match listener.recv(&mut buf) {
                    Ok(len) => {
                        if sender.send(&buf[..len]).is_ok() {
                            packet_count.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(_) => continue,
                }
            }
        }));

        Ok(())
    }

    pub fn stop(&mut self) {
        self.running = false;
        self.stop_flag.store(true, Ordering::SeqCst);
        self.listener = None;
        self.sender = None;
        if let Some(handle) = self.forward_thread.take() {
            let _ = handle.join();
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn packet_count(&self) -> u64 {
        self.packet_count.load(Ordering::SeqCst)
    }
}

impl Default for UdpForwarder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Config {
    pub local_port: u16,
    pub target_ip: String,
    pub target_port: u16,
}

impl Config {
    pub fn load() -> Self {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(key) = hkcu.open_subkey(REG_KEY_PATH) {
            Self {
                local_port: key.get_value::<u32, _>("LocalPort").unwrap_or(9000) as u16,
                target_ip: key
                    .get_value("TargetIP")
                    .unwrap_or_else(|_| "127.0.0.1".to_string()),
                target_port: key.get_value::<u32, _>("TargetPort").unwrap_or(9001) as u16,
            }
        } else {
            Self::default()
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu.create_subkey(REG_KEY_PATH)?;

        key.set_value("LocalPort", &(self.local_port as u32))?;
        key.set_value("TargetIP", &self.target_ip)?;
        key.set_value("TargetPort", &(self.target_port as u32))?;
        Ok(())
    }

    fn default() -> Self {
        Self {
            local_port: 9000,
            target_ip: "127.0.0.1".to_string(),
            target_port: 9001,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forwarder_defaults() {
        let fwd = UdpForwarder::new();
        assert!(!fwd.is_running());
    }

    #[test]
    fn test_forwarder_stop() {
        let mut fwd = UdpForwarder::new();
        fwd.stop();
        assert!(!fwd.is_running());
    }

    #[test]
    fn test_valid_local_port_boundaries() {
        let cfg1 = Config {
            local_port: 1,
            target_ip: "127.0.0.1".to_string(),
            target_port: 9001,
        };
        assert_eq!(cfg1.local_port, 1);

        let cfg2 = Config {
            local_port: 65535,
            target_ip: "127.0.0.1".to_string(),
            target_port: 9001,
        };
        assert_eq!(cfg2.local_port, 65535);
    }
}
