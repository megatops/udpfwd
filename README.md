# UDP Forwarder

A Windows GUI application that forwards UDP packets received on a local port to a specified target IP:Port.

## Features

- **UDP Forwarding**: Receive UDP packets on localhost and forward them verbatim to any target IP:Port
- **Configuration Persistence**: Settings automatically saved to Windows Registry
- **Real-time Statistics**: Display Total packets and PPS in status bar
- **Input Validation**: Validates port numbers and IP addresses before starting
- **System Tray**: Minimize to tray via Windows title bar or tray icon click

## Usage

### GUI Mode

1. Enter the local port to listen on (e.g., 9000)
2. Enter the target IP address (e.g., 127.0.0.1)
3. Enter the target port (e.g., 9001)
4. Click "Start" to begin forwarding
5. Click "Stop" to halt forwarding
6. Use Windows title bar minimize button or click tray icon to hide window

### Command Line

```
udpfwd.exe -l 9000 -i 127.0.0.1 -t 9001 -a
```

Options:

- `-l, --local-port <PORT>`  - Local port to listen on
- `-i, --target-ip <IP>`    - Target IP address
- `-t, --target-port <PORT>` - Target port
- `-a, --auto-start`        - Auto-start forwarding on launch

Command line arguments override registry settings.

## Configuration

Settings are stored in registry at:
`HKEY_CURRENT_USER\Software\Megatops Software\UDP Forwarder`

- LocalPort (DWORD): Local listening port
- TargetIP (String): Target IP address
- TargetPort (DWORD): Target port

## Requirements

- Windows 10 or later
- No additional runtime required (statically linked)

## Building

See BUILD.md for build instructions.

## License

MIT License