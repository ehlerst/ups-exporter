# ups-exporter

A high-performance Prometheus and OpenTelemetry (OTLP) exporter for UPS (Uninterruptible Power Supply) devices, written in Rust. It queries UPS metrics by connecting directly to the local `upsd` daemon over TCP and exposes them via a Prometheus scraper endpoint and/or pushes them periodically to an OpenTelemetry collector.

## Design & Efficiency (Why Rust?)

This exporter is designed to run continuously on low-power, resource-constrained edge hardware such as the Raspberry Pi Zero (ARMv6). Building the exporter in Rust provides significant benefits over traditional implementations:
- **Minimal Resource Footprint**: Running as a daemon on a Raspberry Pi Zero, it consumes **only ~1.0 MB of RAM** (Resident Set Size / RSS) and has a systemd-reported memory usage of **84 KB**. Compare this to Go (~10-20MB) or Python (~15-30MB).
- **Near-Zero CPU Usage**: The exporter uses **less than 0.2% CPU** on the single-core Broadcom BCM2835 processor (ARMv6) of a Raspberry Pi Zero, ensuring system resource consumption remains negligible.
- **Zero Runtime Dependencies**: When compiled with the MUSL target (`arm-unknown-linux-musleabihf`), the binary is statically linked and has no dependencies on external C libraries, meaning you can drop it onto any Linux distribution and it will run immediately.
- **Tiny Binary Size**: The compiled target release binary is only **~600 KiB** in size.
- **High Concurrency Performance**: The multi-threaded Prometheus listener and background OTLP push loops operate with zero overhead.

### Architectural Design: Native TCP Client vs. Tokio Async

Rather than spawning the external `upsc` binary or bringing in heavy async runtimes like `tokio` to talk to the `upsd` server, this exporter implements a native, standard-library-only TCP client that communicates directly with the NUT daemon over TCP port 3493.

This architectural decision has significant resource benefits:
- **No Subprocess Overhead**: Spawning subprocesses (`Command::new("upsc")`) uses significant CPU cycles and creates fork-bomb risks under high scrape concurrency. Communicating directly over TCP loopback is instantaneous and uses zero extra processes.
- **Unmatched Memory Efficiency**: By avoiding async runtimes (`tokio`) and client libraries, we keep our daemon's memory footprint at **only ~1.0 MB of RAM** (Resident Set Size / RSS), with a systemd-reported memory usage of **92 KB**. A typical `tokio`-based daemon requires 10–15 MB just to maintain runtime threads.
- **Ultra-Compact Binary Size**: By using blocking standard library features and keeping dependencies minimal, the compiled static binary size remains under **600 KiB** (compared to 5–8 MB for async equivalents).
- **Stall Protection**: Equipped with 5-second socket timeouts for connection, read, and write operations, making it impossible for the query to hang or block the daemon indefinitely.

## Features

- **Dual Export:** Supports Prometheus scraping (`/metrics` endpoint) and periodic OpenTelemetry (OTLP HTTP JSON) metrics push.
- **Active Power Calculations:** Automatically calculates active power usage from current load percentage and nominal real power rating.
- **Financial Cost Forecasting:** Forecasts hourly, daily, and yearly energy costs in USD based on a configurable kWh rate.
- **Extremely Lightweight:** Compiled as a static Rust binary with minimal resource footprint (ideal for low-power devices like Raspberry Pi Zeros).
- **Systemd Ready:** Includes a pre-configured service template and a Makefile for easy installation.
- **Grafana Dashboard:** Includes a pre-configured dashboard located in `grafana/ups-exporter-dashboard.json`.

## Metrics

| Metric Name | Description | Labels |
| ----------- | ----------- | ------ |
| `ups_battery_charge_percent` | UPS battery charge percentage | `ups` |
| `ups_battery_runtime_seconds` | UPS battery remaining runtime in seconds | `ups` |
| `ups_battery_voltage_volts` | UPS battery voltage | `ups` |
| `ups_input_voltage_volts` | UPS input line voltage | `ups` |
| `ups_output_voltage_volts` | UPS output line voltage | `ups` |
| `ups_load_percent` | UPS load percentage | `ups` |
| `ups_realpower_nominal_watts` | UPS nominal real power rating | `ups` |
| `ups_power_active_watts` | Calculated active power usage of the load | `ups` |
| `ups_cost_hourly_USD` | Calculated hourly electricity cost | `ups` |
| `ups_cost_daily_USD` | Calculated daily electricity cost | `ups` |
| `ups_cost_yearly_USD` | Calculated yearly electricity cost | `ups` |

## Requirements

- **Network UPS Tools (`nut`)** daemon (`upsd`) and drivers installed and configured on the server (running on port `3493`).
- **Rust Toolchain** (if compiling from source).

### Installing and Configuring NUT

On Debian/Ubuntu-based systems, you can install and configure NUT as follows:

1. **Install NUT packages**:
   ```bash
   sudo apt update
   sudo apt install nut
   ```

2. **Configure the driver for your USB UPS**:
   Add the driver configuration to `/etc/nut/ups.conf`. For USB-based CyberPower or other USB HID UPS systems, use the `usbhid-ups` driver:
   ```ini
   [cyberpower]
       driver = usbhid-ups
       port = auto
       desc = "Cyber Power System PR1500LCDRT2U"
   ```

3. **Configure the NUT mode**:
   Edit `/etc/nut/nut.conf` and set the mode to `standalone`:
   ```ini
   MODE=standalone
   ```

4. **Start the services**:
   ```bash
   sudo upsdrvctl start
   sudo systemctl restart nut-server nut-client
   ```

5. **Verify the installation**:
   Verify that the `upsd` daemon is responding on port `3493` using Python or Netcat:
   ```bash
   python3 -c "import socket; s = socket.socket(); s.connect(('127.0.0.1', 3493)); s.sendall(b'LIST VAR cyberpower\n'); print(s.recv(1024).decode())"
   ```

## Installation

### 1. Build from Source
```bash
make build
sudo make install
```

### 2. Setup as a Service
```bash
sudo make setup-service
```

## Configuration

The exporter is configured via `/etc/default/ups-exporter`.

To modify options such as the Prometheus port, the target UPS name, the kWh rate, or OTLP endpoints:

1. Edit `/etc/default/ups-exporter`:
```bash
# /etc/default/ups-exporter
UPS_EXPORTER_OPTS="-prom-port 9102 -ups-name cyberpower -ups-label gamer -kwh-rate 0.15 -otlp-endpoint http://prom.k.net:4318/v1/metrics"
```

2. Restart the service:
```bash
sudo systemctl restart ups-exporter
```

### Command Line Flags

The following flags are available on the `ups-exporter` binary:

- `-prom-port <port>`: Port for Prometheus metrics (default: `9102`)
- `-enable-prom <true|false>`: Enable/disable Prometheus scraper endpoint (default: `true`)
- `-otlp-endpoint <url>`: OTLP HTTP receiver endpoint (e.g. `http://prom.k.net:4318/v1/metrics`)
- `-ups-name <name>`: Name of the UPS to query via upsc (default: `cyberpower`)
- `-ups-label <label>`: Label name for the UPS device in metrics (default: `gamer`)
- `-kwh-rate <rate>`: USD electricity rate per kWh (default: `0.15`)
- `-interval <secs>`: OTLP metric push interval in seconds (default: `15`)
- `-debug`: Enable verbose debug logging
- `-h, --help`: Print the help menu

## Prometheus Configuration

To scrape metrics directly from the exporter, add the following job to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'ups-exporter'
    static_configs:
      - targets: ['localhost:9102']
    scrape_interval: 15s
```

## OpenTelemetry Configuration (OTLP)

If you prefer pushing metrics to an OTel Collector, configure the OTLP flag in `/etc/default/ups-exporter`:

```bash
UPS_EXPORTER_OPTS="-otlp-endpoint http://my-otel-collector:4318/v1/metrics -interval 15"
```

## License

Apache License 2.0
