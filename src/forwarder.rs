// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Ding Zhaojie <zhaojie_ding@msn.com>

//! High-performance IPv4 UDP packet forwarding engine.
//!
//! [`UdpForwarder`] binds a single IPv4 UDP socket and spawns a dedicated thread
//! that receives packets and forwards them verbatim to the configured target.
//! Blocking I/O in a tight loop minimizes latency and maximizes throughput.
//!
//! [`Config`] persists forwarding settings to the Windows registry under
//! `HKEY_CURRENT_USER\Software\Megatops Software\UDP Forwarder`.

use std::net::{SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

const REGISTRY_KEY_PATH: &str = r"Software\Megatops Software\UDP Forwarder";
const BUFFER_SIZE: usize = 65535;
const READ_TIMEOUT_MS: u64 = 100;

// -----------------------------------------------------------------------------
// Public API
// -----------------------------------------------------------------------------

/// Resolves a hostname and port into an IPv4 [`SocketAddrV4`].
///
/// Accepts dotted-quad IPv4 addresses and hostnames. IPv6 results are filtered
/// out; only the first IPv4 address is returned.
///
/// # Errors
///
/// Returns a descriptive error if resolution fails or no IPv4 address is found.
pub fn resolve_target(host: &str, port: u16) -> Result<SocketAddrV4, String> {
    let target = format!("{host}:{port}");
    target
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve \"{target}\": {e}"))?
        .filter_map(|addr| match addr {
            SocketAddr::V4(v4) => Some(v4),
            SocketAddr::V6(_) => None,
        })
        .next()
        .ok_or_else(|| format!("no IPv4 address for \"{target}\""))
}

// -----------------------------------------------------------------------------
// UdpForwarder
// -----------------------------------------------------------------------------

/// UDP packet forwarder backed by a dedicated I/O thread.
///
/// Binds a single IPv4 listener and forwards all received packets to the
/// configured target. The forwarding thread uses blocking I/O in a tight loop
/// for maximum throughput.
///
/// # Thread Safety
///
/// All public methods are safe to call from any thread. The hot path (packet
/// forwarding) uses only atomic operations — no mutex on the forwarding loop.
pub struct UdpForwarder {
    packet_count: Arc<AtomicU64>,
    stop_flag: Arc<AtomicBool>,
    forward_thread: Option<thread::JoinHandle<()>>,
    config: Option<(u16, SocketAddrV4)>,
}

impl UdpForwarder {
    /// Creates a new forwarder in the idle state.
    #[inline]
    pub fn new() -> Self {
        Self {
            packet_count: Arc::new(AtomicU64::new(0)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            forward_thread: None,
            config: None,
        }
    }

    /// Binds the listener socket and spawns the forwarding thread.
    ///
    /// The listener binds to `0.0.0.0:{local_port}`. A connected sender socket
    /// forwards each received datagram to `target_addr`.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if either socket cannot be bound or connected.
    pub fn start(&mut self, local_port: u16, target_addr: SocketAddrV4) -> std::io::Result<()> {
        let listener = UdpSocket::bind(format!("0.0.0.0:{local_port}"))?;
        let sender = UdpSocket::bind("0.0.0.0:0")?;
        sender.connect(SocketAddr::V4(target_addr))?;

        // Set a short timeout so the loop can check the stop flag regularly
        listener.set_read_timeout(Some(std::time::Duration::from_millis(READ_TIMEOUT_MS)))?;

        self.packet_count.store(0, Ordering::SeqCst);
        self.stop_flag.store(false, Ordering::SeqCst);
        self.config = Some((local_port, target_addr));

        let packet_count = Arc::clone(&self.packet_count);
        let stop_flag = Arc::clone(&self.stop_flag);

        self.forward_thread = Some(thread::spawn(move || {
            let mut buf = [0u8; BUFFER_SIZE];
            loop {
                if stop_flag.load(Ordering::SeqCst) {
                    break;
                }

                match listener.recv_from(&mut buf) {
                    Ok((len, _)) => {
                        if sender.send(&buf[..len]).is_ok() {
                            packet_count.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(_) => continue,
                }
            }
            drop(listener);
            drop(sender);
        }));

        Ok(())
    }

    /// Signals the forwarding thread to stop and waits for it to exit.
    ///
    /// Idempotent — safe to call multiple times.
    #[inline]
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.forward_thread.take() {
            let _ = handle.join();
        }
        self.config = None;
    }

    /// Restarts forwarding with the previous configuration.
    ///
    /// Used to recover after Windows power-resume events.
    ///
    /// # Errors
    ///
    /// Returns an error if no configuration exists from a prior [`start()`].
    pub fn restart(&mut self) -> std::io::Result<()> {
        let (local_port, target_addr) = self
            .config
            .ok_or_else(|| std::io::Error::other("no config for restart"))?;
        self.stop();
        self.start(local_port, target_addr)
    }

    /// Returns `true` if the forwarder has saved configuration for restart.
    #[inline]
    #[cfg(test)]
    pub fn has_config(&self) -> bool {
        self.config.is_some()
    }

    /// Returns the total number of packets forwarded since [`start()`].
    #[inline]
    pub fn packet_count(&self) -> u64 {
        self.packet_count.load(Ordering::SeqCst)
    }

    /// Returns `true` if the forwarding thread is running.
    #[inline]
    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.forward_thread.is_some()
    }
}

impl Default for UdpForwarder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Forwarding configuration persisted to the Windows registry.
///
/// Registry path: `HKEY_CURRENT_USER\Software\Megatops Software\UDP Forwarder`
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Local UDP port to listen on.
    pub local_port: u16,
    /// Target IP address or hostname.
    pub target_ip: String,
    /// Target UDP port.
    pub target_port: u16,
}

impl Config {
    const DEFAULT_LOCAL_PORT: u16 = 8888;
    const DEFAULT_TARGET_IP: &str = "192.168.0.1";
    const DEFAULT_TARGET_PORT: u16 = 8888;

    /// Loads configuration from the registry, falling back to defaults.
    ///
    /// Defaults: local port 8888, target IP 192.168.0.1, target port 8888.
    pub fn load() -> Self {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        match hkcu.open_subkey(REGISTRY_KEY_PATH) {
            Ok(key) => Self {
                local_port: key
                    .get_value::<u32, _>("LocalPort")
                    .unwrap_or(Self::DEFAULT_LOCAL_PORT as u32) as u16,
                target_ip: key
                    .get_value("TargetIP")
                    .unwrap_or_else(|_| Self::DEFAULT_TARGET_IP.to_string()),
                target_port: key
                    .get_value::<u32, _>("TargetPort")
                    .unwrap_or(Self::DEFAULT_TARGET_PORT as u32)
                    as u16,
            },
            Err(_) => Self::default(),
        }
    }

    /// Saves configuration to the registry, creating the key if needed.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the registry key cannot be created or written.
    pub fn save(&self) -> std::io::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu.create_subkey(REGISTRY_KEY_PATH)?;
        key.set_value("LocalPort", &(self.local_port as u32))?;
        key.set_value("TargetIP", &self.target_ip)?;
        key.set_value("TargetPort", &(self.target_port as u32))?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            local_port: Self::DEFAULT_LOCAL_PORT,
            target_ip: Self::DEFAULT_TARGET_IP.to_string(),
            target_port: Self::DEFAULT_TARGET_PORT,
        }
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn forwarder_defaults_to_idle() {
        let fwd = UdpForwarder::new();
        assert_eq!(fwd.packet_count(), 0);
        assert!(!fwd.is_running());
        assert!(!fwd.has_config());
    }

    #[test]
    fn forwarder_stop_is_idempotent() {
        let mut fwd = UdpForwarder::new();
        fwd.stop();
        fwd.stop();
        assert_eq!(fwd.packet_count(), 0);
    }

    #[test]
    fn config_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.local_port, 8888);
        assert_eq!(cfg.target_ip, "192.168.0.1");
        assert_eq!(cfg.target_port, 8888);
    }

    #[test]
    fn forwarder_has_config_after_start() {
        let mut fwd = UdpForwarder::new();
        let addr = resolve_target("127.0.0.1", 19001).unwrap();
        let _ = fwd.start(19000, addr);
        assert!(fwd.has_config());
        fwd.stop();
        assert!(!fwd.has_config());
    }

    #[test]
    fn forwarder_restart_rebuilds_socket() {
        let mut fwd = UdpForwarder::new();
        let addr = resolve_target("127.0.0.1", 19101).unwrap();
        let _ = fwd.start(19010, addr.clone());
        let _ = fwd.restart();
        assert_eq!(fwd.packet_count(), 0);
        assert!(fwd.is_running());
        fwd.stop();
    }

    #[test]
    fn forwarder_restart_fails_without_config() {
        let mut fwd = UdpForwarder::new();
        assert!(fwd.restart().is_err());
    }

    #[test]
    fn resolve_ipv4_loopback() {
        let addr = resolve_target("127.0.0.1", 9001).unwrap();
        assert_eq!(addr.ip(), &Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(addr.port(), 9001);
    }

    #[test]
    fn resolve_hostname_localhost() {
        let addr = resolve_target("localhost", 9001).unwrap();
        assert_eq!(addr.port(), 9001);
    }

    #[test]
    fn resolve_invalid_host_fails() {
        assert!(resolve_target("invalid.host", 9001).is_err());
    }
}
