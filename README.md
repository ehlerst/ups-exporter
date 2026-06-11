# ups-exporter

A high-performance Prometheus and OpenTelemetry (OTLP) exporter for UPS (Uninterruptible Power Supply) devices, written in Rust. It queries UPS metrics using the local `upsc` utility and exposes them via a Prometheus scraper endpoint and/or pushes them periodically to an OpenTelemetry collector.

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

- **Network UPS Tools (`nut`)** installed and configured, with a running UPS daemon (`upsd`).
- **`upsc`** CLI utility installed locally.
- **Rust Toolchain** (if compiling from source).

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
