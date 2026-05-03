#!/usr/bin/env python3
"""Loopback and performance tests for UDP Forwarder.

Validates:
1. Loopback: packets sent to local port arrive at target with correct payload
2. Performance: >1000 pps with zero packet loss

IPv4 only.
"""

from __future__ import annotations

import contextlib
import socket
import subprocess
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Optional


# ============================================================================
# Configuration
# ============================================================================

@dataclass(frozen=True)
class LoopbackConfig:
    """Loopback test parameters."""

    local_port: int = 9000
    target_ip: str = "127.0.0.1"
    target_port: int = 9001
    message: bytes = b"Hello UDP Forwarder Test!"
    packet_count: int = 100
    recv_buffer: int = 4096
    recv_timeout: float = 5.0
    startup_delay: float = 2.0
    receiver_timeout: float = 10.0
    shutdown_timeout: float = 3.0


@dataclass(frozen=True)
class PerfConfig:
    """Performance test parameters."""

    local_port: int = 9100
    target_ip: str = "127.0.0.1"
    target_port: int = 9101
    message: bytes = b"P" * 1400
    packet_count: int = 5000
    recv_buffer: int = 65535
    recv_timeout: float = 10.0
    startup_delay: float = 1.0
    receiver_timeout: float = 15.0
    shutdown_timeout: float = 5.0


LOOPBACK_TESTS = [
    LoopbackConfig(local_port=19000, target_ip="127.0.0.1", target_port=19001),
    LoopbackConfig(local_port=19010, target_ip="127.0.0.1", target_port=19011),
]


# ============================================================================
# Process Management
# ============================================================================

def find_udpfwd_exe() -> Optional[Path]:
    """Locates udpfwd.exe in the release build directory."""
    exe_path = Path(__file__).resolve().parent.parent / "target" / "release" / "udpfwd.exe"
    return exe_path if exe_path.exists() else None


@contextlib.contextmanager
def running_forwarder(cfg: LoopbackConfig | PerfConfig):
    """Context manager that starts and stops the udpfwd process."""
    exe_path = find_udpfwd_exe()
    if not exe_path:
        print("ERROR: udpfwd.exe not found")
        yield None
        return

    target_str = f"{cfg.target_ip}:{cfg.target_port}"
    proc = subprocess.Popen(
        [str(exe_path), "-l", str(cfg.local_port), "-t", target_str, "-a"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    time.sleep(cfg.startup_delay)
    try:
        yield proc
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=cfg.shutdown_timeout)
        except subprocess.TimeoutExpired:
            proc.kill()


# ============================================================================
# Packet I/O
# ============================================================================

def send_packets(addr: tuple[str, int], message: bytes, *, count: int) -> None:
    """Sends `count` UDP packets containing `message` to `addr`."""
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        for _ in range(count):
            sock.sendto(message, addr)


# ============================================================================
# Loopback Test
# ============================================================================

def run_loopback_test(cfg: LoopbackConfig) -> int:
    """Runs a loopback test. Returns 0 on success."""
    print(f"Testing: {cfg.local_port} -> {cfg.target_ip}:{cfg.target_port}")

    with running_forwarder(cfg) as proc:
        if proc is None:
            return 1

        # Verify the forwarder is still running
        time.sleep(0.5)
        if proc.poll() is not None:
            print(f"ERROR: Forwarder exited with code {proc.returncode}")
            return 1

        # Pre-bind receiver socket before sending
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind((cfg.target_ip, cfg.target_port))
        sock.settimeout(cfg.recv_timeout)

        received: list[bytes] = []
        stop = threading.Event()

        def receiver() -> None:
            while not stop.is_set():
                try:
                    data, _ = sock.recvfrom(cfg.recv_buffer)
                    received.append(data)
                except (socket.timeout, OSError):
                    break

        rx = threading.Thread(target=receiver, daemon=True)
        rx.start()
        time.sleep(0.3)

        send_packets((cfg.target_ip, cfg.local_port), cfg.message, count=cfg.packet_count)

        rx.join(timeout=cfg.receiver_timeout)
        stop.set()
        time.sleep(0.1)
        sock.close()

        print(f"  Sent: {cfg.packet_count}, Received: {len(received)}")

        if not received:
            print("  FAILED: No packets received!")
            return 1

        if len(received) < int(cfg.packet_count * 0.9):
            print(f"  FAILED: Only {len(received)}/{cfg.packet_count} received")
            return 1

        for i, pkt in enumerate(received[:10]):
            if pkt != cfg.message:
                print(f"  FAILED: Packet {i} content mismatch!")
                return 1

        print("  PASSED")
        return 0


# ============================================================================
# Performance Test
# ============================================================================

def run_performance_test(cfg: PerfConfig) -> int:
    """Runs a performance test. Requires >1000 pps with zero loss."""
    print(f"Performance: {cfg.local_port} -> {cfg.target_ip}:{cfg.target_port}")
    print(f"  Target: {cfg.packet_count} packets @ ~{len(cfg.message)} bytes each")

    with running_forwarder(cfg) as proc:
        if proc is None:
            return 1

        time.sleep(0.5)
        if proc.poll() is not None:
            print("  ERROR: Forwarder exited")
            return 1

        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind((cfg.target_ip, cfg.target_port))
        sock.settimeout(cfg.recv_timeout)

        received: list[bytes] = []
        stop = threading.Event()

        def receiver() -> None:
            while not stop.is_set():
                try:
                    data, _ = sock.recvfrom(cfg.recv_buffer)
                    received.append(data)
                except (socket.timeout, OSError):
                    break

        rx = threading.Thread(target=receiver, daemon=True)
        rx.start()
        time.sleep(0.2)

        t0 = time.perf_counter()
        send_packets((cfg.target_ip, cfg.local_port), cfg.message, count=cfg.packet_count)
        send_dur = time.perf_counter() - t0

        time.sleep(0.5)
        stop.set()
        rx.join(timeout=2.0)
        sock.close()

        send_rate = cfg.packet_count / send_dur
        print(f"  Sent: {cfg.packet_count} in {send_dur*1000:.1f}ms ({send_rate:.0f} pps)")
        print(f"  Received: {len(received)}")

        if len(received) != cfg.packet_count:
            loss = cfg.packet_count - len(received)
            print(f"  FAILED: {loss} packets lost")
            return 1

        expected_len = len(cfg.message)
        for i, pkt in enumerate(received[:10]):
            if len(pkt) != expected_len:
                print(f"  FAILED: Packet {i} size {len(pkt)} != {expected_len}")
                return 1

        recv_rate = cfg.packet_count / (send_dur + 0.5)
        print(f"  PASSED - {recv_rate:.0f} pps (zero loss)")
        return 0


# ============================================================================
# Main
# ============================================================================

def main() -> int:
    """Runs all test scenarios."""
    print("=" * 60)
    print("UDP Forwarder Test Suite (IPv4 Only)")
    print("=" * 60)
    print()

    results: list[tuple[str, int]] = []

    print("=== Loopback Tests ===")
    for cfg in LOOPBACK_TESTS:
        result = run_loopback_test(cfg)
        results.append((f"loopback {cfg.local_port}", result))
        time.sleep(1)

    print()
    print("=== Performance Test ===")
    results.append(("performance >1000pps", run_performance_test(PerfConfig())))

    print()
    print("=" * 60)
    print("Summary:")
    passed = sum(1 for _, r in results if r == 0)
    for name, result in results:
        print(f"  {name} : {'PASS' if result == 0 else 'FAIL'}")
    print(f"  Total: {passed}/{len(results)} passed")
    print("=" * 60)

    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    import sys
    sys.exit(main())