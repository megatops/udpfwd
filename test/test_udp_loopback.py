#!/usr/bin/env python3
"""UDP loopback test for udpfwd"""

import socket
import subprocess
import sys
import time
import threading
import os
import signal

LOCAL_PORT = 9000
TARGET_IP = "127.0.0.1"
TARGET_PORT = 9001
TEST_MESSAGE = b"Hello UDP Forwarder Test!"
PACKET_COUNT = 100

def start_forwarder():
    """Start udpfwd with auto-start"""
    udpfwd_path = os.path.join(os.path.dirname(os.path.dirname(__file__)), "target", "release", "udpfwd.exe")
    if not os.path.exists(udpfwd_path):
        print(f"ERROR: udpfwd.exe not found at {udpfwd_path}")
        return None
    
    proc = subprocess.Popen(
        [udpfwd_path, "-l", str(LOCAL_PORT), "-i", TARGET_IP, "-t", str(TARGET_PORT), "-a"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        creationflags=subprocess.CREATE_NEW_CONSOLE if os.name == 'nt' else 0
    )
    time.sleep(1)  # Wait for startup
    return proc

def stop_forwarder(proc):
    """Stop udpfwd"""
    if proc:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()

def receiver():
    """Receive packets on target port"""
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind((TARGET_IP, TARGET_PORT))
    sock.settimeout(5.0)
    
    received = []
    try:
        while len(received) < PACKET_COUNT:
            data, addr = sock.recvfrom(4096)
            received.append(data)
            if len(received) >= PACKET_COUNT:
                break
    except socket.timeout:
        pass
    sock.close()
    return received

def sender(count):
    """Send packets to local port"""
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    
    try:
        for i in range(count):
            sock.sendto(TEST_MESSAGE, ("127.0.0.1", LOCAL_PORT))
    finally:
        sock.close()

def run_receiver():
    global received_packets
    received_packets = receiver()

def main():
    global received_packets
    
    print(f"Starting UDP loopback test...")
    print(f"Local port: {LOCAL_PORT}, Target: {TARGET_IP}:{TARGET_PORT}")
    
    # Start udpfwd
    print("Starting udpfwd...")
    proc = start_forwarder()
    if not proc:
        return 1
    
    try:
        print(f"Sending {PACKET_COUNT} packets...")
        
        # Start receiver in background
        received_packets = []
        receiver_thread = threading.Thread(target=run_receiver)
        receiver_thread.start()
        
        # Send packets
        sender(PACKET_COUNT)
        
        # Wait for receiver
        receiver_thread.join(timeout=10)
        
        # Results
        print(f"\nResults:")
        print(f"  Sent: {PACKET_COUNT}")
        print(f"  Received: {len(received_packets)}")
        
        if len(received_packets) == 0:
            print("\nFAILED: No packets received!")
            return 1
        
        if len(received_packets) < PACKET_COUNT:
            print(f"\nPARTIAL: Only {len(received_packets)}/{PACKET_COUNT} packets received")
            return 1
        
        # Verify packet content
        for i, pkt in enumerate(received_packets):
            if pkt != TEST_MESSAGE:
                print(f"\nFAILED: Packet {i} content mismatch!")
                return 1
        
        print("\nPASSED: All packets received correctly!")
        
    finally:
        print("Stopping udpfwd...")
        stop_forwarder(proc)
    
    return 0

if __name__ == "__main__":
    sys.exit(main())