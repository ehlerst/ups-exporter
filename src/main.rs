use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use serde_json::json;
use std::net::TcpListener;
use std::io::{Read, Write, BufReader, BufRead};
use std::thread;
use std::time::Duration;
use std::net::TcpStream;

fn print_help() {
    println!("UPS Exporter {}", env!("CARGO_PKG_VERSION"));
    println!("Usage: ups-exporter [options]");
    println!();
    println!("Options:");
    println!("  -prom-port <port>       Port for Prometheus metrics (default: 9102)");
    println!("  -enable-prom <true|false> Enable/disable Prometheus endpoint (default: true)");
    println!("  -otlp-endpoint <url>    OTLP HTTP receiver endpoint (e.g. http://prom.k.net:4318/v1/metrics)");
    println!("  -ups-name <name>        Name of the UPS to query via upsc (default: cyberpower)");
    println!("  -ups-label <label>      Label name for the UPS device in metrics (default: gamer)");
    println!("  -kwh-rate <rate>        USD electricity rate per kWh (default: 0.15)");
    println!("  -interval <secs>        OTLP metric push interval in seconds (default: 15)");
    println!("  -debug                  Enable verbose debug logging");
    println!("  -h, --help              Print this help menu");
}

fn get_ups_metrics(ups_name: &str) -> Result<HashMap<String, String>, Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = TcpStream::connect_timeout(
        &"127.0.0.1:3493".parse()?,
        Duration::from_secs(5)
    )?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let command = format!("LIST VAR {}\n", ups_name);
    stream.write_all(command.as_bytes())?;
    stream.flush()?;

    let reader = BufReader::new(stream);
    let mut metrics = HashMap::new();
    let mut in_list = false;

    for line in reader.lines() {
        let line = line?;
        if line.starts_with("BEGIN LIST VAR") {
            in_list = true;
            continue;
        }
        if line.starts_with("END LIST VAR") {
            break;
        }
        if in_list && line.starts_with("VAR ") {
            let parts: Vec<&str> = line.splitn(4, ' ').collect();
            if parts.len() >= 4 {
                let var_name = parts[2].to_string();
                let mut var_value = parts[3].to_string();
                if var_value.starts_with('"') && var_value.ends_with('"') {
                    var_value.pop();
                    var_value.remove(0);
                }
                metrics.insert(var_name, var_value);
            }
        }
    }
    
    if metrics.is_empty() {
        return Err("No metrics received from upsd".into());
    }

    Ok(metrics)
}

fn get_hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "pizero".to_string())
}

fn generate_prometheus_metrics(metrics: &HashMap<String, String>, ups_label: &str, kwh_rate: f64) -> String {
    let mut out = String::new();
    
    // Define mappings: (Prometheus name, upsc key, description)
    let mappings = vec![
        ("ups_battery_charge_percent", "battery.charge", "UPS battery charge percentage"),
        ("ups_battery_runtime_seconds", "battery.runtime", "UPS battery remaining runtime in seconds"),
        ("ups_battery_voltage_volts", "battery.voltage", "UPS battery voltage"),
        ("ups_input_voltage_volts", "input.voltage", "UPS input line voltage"),
        ("ups_output_voltage_volts", "output.voltage", "UPS output line voltage"),
        ("ups_load_percent", "ups.load", "UPS load percentage"),
        ("ups_realpower_nominal_watts", "ups.realpower.nominal", "UPS nominal real power rating"),
    ];

    for (name, key, desc) in mappings {
        if let Some(val_str) = metrics.get(key) {
            if let Ok(val_float) = val_str.parse::<f64>() {
                out.push_str(&format!("# HELP {} {}\n", name, desc));
                out.push_str(&format!("# TYPE {} gauge\n", name));
                out.push_str(&format!("{}{{ups=\"{}\"}} {}\n", name, ups_label, val_float));
            }
        }
    }

    // Calculated active power usage and financial cost metrics
    if let (Some(load_str), Some(nominal_str)) = (metrics.get("ups.load"), metrics.get("ups.realpower.nominal")) {
        if let (Ok(load), Ok(nominal)) = (load_str.parse::<f64>(), nominal_str.parse::<f64>()) {
            if load >= 0.0 && nominal > 0.0 {
                let active_power = (load / 100.0) * nominal; // Watts
                
                out.push_str("# HELP ups_power_active_watts Calculated active power usage of the load\n");
                out.push_str("# TYPE ups_power_active_watts gauge\n");
                out.push_str(&format!("ups_power_active_watts{{ups=\"{}\"}} {}\n", ups_label, active_power));

                // Calculate energy costs
                let kwh = active_power / 1000.0;
                let cost_hourly = kwh * kwh_rate;
                let cost_daily = cost_hourly * 24.0;
                let cost_yearly = cost_daily * 365.0;

                out.push_str("# HELP ups_cost_hourly_USD Calculated hourly electricity cost\n");
                out.push_str("# TYPE ups_cost_hourly_USD gauge\n");
                out.push_str(&format!("ups_cost_hourly_USD{{ups=\"{}\"}} {}\n", ups_label, cost_hourly));

                out.push_str("# HELP ups_cost_daily_USD Calculated daily electricity cost\n");
                out.push_str("# TYPE ups_cost_daily_USD gauge\n");
                out.push_str(&format!("ups_cost_daily_USD{{ups=\"{}\"}} {}\n", ups_label, cost_daily));

                out.push_str("# HELP ups_cost_yearly_USD Calculated yearly electricity cost\n");
                out.push_str("# TYPE ups_cost_yearly_USD gauge\n");
                out.push_str(&format!("ups_cost_yearly_USD{{ups=\"{}\"}} {}\n", ups_label, cost_yearly));
            }
        }
    }

    out
}

fn push_otlp_metrics(
    otlp_endpoint: &str,
    metrics: &HashMap<String, String>,
    ups_label: &str,
    kwh_rate: f64,
    debug: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now_nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos().to_string();
    let dp_attribs = json!([{"key": "ups", "value": {"stringValue": ups_label}}]);

    // Define mappings: (OTLP metric name, upsc key, type, unit, description)
    let mappings = vec![
        ("ups.battery.charge", "battery.charge", "double", "%", "UPS battery charge percentage"),
        ("ups.battery.runtime", "battery.runtime", "int", "s", "UPS battery remaining runtime in seconds"),
        ("ups.battery.voltage", "battery.voltage", "double", "V", "UPS battery voltage"),
        ("ups.input.voltage", "input.voltage", "double", "V", "UPS input line voltage"),
        ("ups.output.voltage", "output.voltage", "double", "V", "UPS output line voltage"),
        ("ups.load", "ups.load", "double", "%", "UPS load percentage"),
        ("ups.realpower.nominal", "ups.realpower.nominal", "int", "W", "UPS nominal real power rating"),
    ];

    let mut otlp_metrics = Vec::new();

    for (name, key, val_type, unit, desc) in mappings {
        if let Some(val_str) = metrics.get(key) {
            if let Ok(val_float) = val_str.parse::<f64>() {
                let mut dp = json!({
                    "timeUnixNano": now_nanos,
                    "attributes": dp_attribs
                });

                if val_type == "double" {
                    dp["asDouble"] = json!(val_float);
                } else {
                    dp["asInt"] = json!((val_float as i64).to_string());
                }

                otlp_metrics.push(json!({
                    "name": name,
                    "description": desc,
                    "unit": unit,
                    "gauge": {
                        "dataPoints": [dp]
                    }
                }));
            }
        }
    }

    // Calculated active power usage and financial cost metrics
    if let (Some(load_str), Some(nominal_str)) = (metrics.get("ups.load"), metrics.get("ups.realpower.nominal")) {
        if let (Ok(load), Ok(nominal)) = (load_str.parse::<f64>(), nominal_str.parse::<f64>()) {
            if load >= 0.0 && nominal > 0.0 {
                let active_power = (load / 100.0) * nominal; // Watts
                
                otlp_metrics.push(json!({
                    "name": "ups.power.active",
                    "description": "Calculated active power usage of the load",
                    "unit": "W",
                    "gauge": {
                        "dataPoints": [{
                            "timeUnixNano": now_nanos,
                            "asDouble": active_power,
                            "attributes": dp_attribs
                        }]
                    }
                }));

                // Calculate energy costs
                let kwh = active_power / 1000.0;
                let cost_hourly = kwh * kwh_rate;
                let cost_daily = cost_hourly * 24.0;
                let cost_yearly = cost_daily * 365.0;

                otlp_metrics.push(json!({
                    "name": "ups.cost.hourly",
                    "description": "Calculated hourly electricity cost",
                    "unit": "USD",
                    "gauge": {
                        "dataPoints": [{
                            "timeUnixNano": now_nanos,
                            "asDouble": cost_hourly,
                            "attributes": dp_attribs
                        }]
                    }
                }));

                otlp_metrics.push(json!({
                    "name": "ups.cost.daily",
                    "description": "Calculated daily electricity cost",
                    "unit": "USD",
                    "gauge": {
                        "dataPoints": [{
                            "timeUnixNano": now_nanos,
                            "asDouble": cost_daily,
                            "attributes": dp_attribs
                        }]
                    }
                }));

                otlp_metrics.push(json!({
                    "name": "ups.cost.yearly",
                    "description": "Calculated yearly electricity cost",
                    "unit": "USD",
                    "gauge": {
                        "dataPoints": [{
                            "timeUnixNano": now_nanos,
                            "asDouble": cost_yearly,
                            "attributes": dp_attribs
                        }]
                    }
                }));
            }
        }
    }

    if otlp_metrics.is_empty() {
        if debug {
            println!("No metrics parsed for OTLP payload.");
        }
        return Ok(());
    }

    let payload = json!({
        "resourceMetrics": [{
            "resource": {
                "attributes": [
                    {"key": "service.name", "value": {"stringValue": "ups-monitor"}},
                    {"key": "host.name", "value": {"stringValue": get_hostname()}},
                    {"key": "ups.name", "value": {"stringValue": ups_label}}
                ]
            },
            "scopeMetrics": [{
                "scope": {"name": "ups.metrics"},
                "metrics": otlp_metrics
            }]
        }]
    });

    if debug {
        println!("Sending OTLP payload to {}", otlp_endpoint);
    }

    match ureq::post(otlp_endpoint)
        .set("Content-Type", "application/json")
        .send_json(&payload)
    {
        Ok(response) => {
            if debug {
                println!(
                    "OTLP push successful. Status: {}, Response: {}",
                    response.status(),
                    response.into_string().unwrap_or_default()
                );
            }
            Ok(())
        }
        Err(e) => Err(format!("OTLP push request failed: {}", e).into()),
    }
}

fn start_otlp_push_loop(
    otlp_endpoint: String,
    ups_name: String,
    ups_label: String,
    kwh_rate: f64,
    interval_secs: u64,
    debug: bool,
) {
    if otlp_endpoint.is_empty() {
        if debug {
            println!("OTLP endpoint is empty. OTLP push is disabled.");
        }
        return;
    }

    if debug {
        println!(
            "Starting background OTLP push loop targeting {} every {} seconds",
            otlp_endpoint, interval_secs
        );
    }

    thread::spawn(move || loop {
        match get_ups_metrics(&ups_name) {
            Ok(metrics) => {
                if let Err(e) = push_otlp_metrics(&otlp_endpoint, &metrics, &ups_label, kwh_rate, debug) {
                    eprintln!("Error pushing OTLP metrics: {}", e);
                }
            }
            Err(e) => {
                eprintln!("Error querying UPS metrics for OTLP push: {}", e);
            }
        }
        thread::sleep(std::time::Duration::from_secs(interval_secs));
    });
}

fn run_prometheus_server_blocking(
    port: u16,
    ups_name: String,
    ups_label: String,
    kwh_rate: f64,
    debug: bool,
) {
    let listener = match TcpListener::bind(format!("0.0.0.0:{}", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to Prometheus port {}: {}", port, e);
            std::process::exit(1);
        }
    };

    println!("Prometheus metrics server running on port {}", port);

    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };
        let ups_name = ups_name.clone();
        let ups_label = ups_label.clone();
        
        thread::spawn(move || {
            let mut buffer = [0; 1024];
            let _ = stream.read(&mut buffer);
            let req_str = String::from_utf8_lossy(&buffer);
            
            if req_str.starts_with("GET /metrics") {
                if debug {
                    println!("Prometheus server: GET /metrics");
                }
                let metrics_data = match get_ups_metrics(&ups_name) {
                    Ok(metrics) => generate_prometheus_metrics(&metrics, &ups_label, kwh_rate),
                    Err(e) => {
                        eprintln!("Error generating metrics for scrape: {}", e);
                        format!("# ERROR querying UPS: {}\n", e)
                    }
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    metrics_data.len(),
                    metrics_data
                );
                let _ = stream.write_all(response.as_bytes());
            } else {
                let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nNot Found";
                let _ = stream.write_all(response.as_bytes());
            }
            let _ = stream.flush();
        });
    }
}

fn main() {
    let mut prom_port = 9102;
    let mut enable_prom = true;
    let mut otlp_endpoint = String::new();
    let mut ups_name = "cyberpower".to_string();
    let mut ups_label = "gamer".to_string();
    let mut kwh_rate = 0.15;
    let mut interval = 15;
    let mut debug = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-prom-port" => {
                if let Some(val) = args.next() {
                    prom_port = val.parse().unwrap_or(9102);
                }
            }
            "-enable-prom" => {
                if let Some(val) = args.next() {
                    enable_prom = val.parse().unwrap_or(true);
                }
            }
            "-otlp-endpoint" => {
                if let Some(val) = args.next() {
                    otlp_endpoint = val;
                }
            }
            "-ups-name" => {
                if let Some(val) = args.next() {
                    ups_name = val;
                }
            }
            "-ups-label" => {
                if let Some(val) = args.next() {
                    ups_label = val;
                }
            }
            "-kwh-rate" => {
                if let Some(val) = args.next() {
                    kwh_rate = val.parse().unwrap_or(0.15);
                }
            }
            "-interval" => {
                if let Some(val) = args.next() {
                    interval = val.parse().unwrap_or(15);
                }
            }
            "-debug" => {
                debug = true;
            }
            "-h" | "--help" | "-help" => {
                print_help();
                std::process::exit(0);
            }
            _ => {
                eprintln!("Unknown argument: {}", arg);
                print_help();
                std::process::exit(1);
            }
        }
    }

    if debug {
        println!("Configuration loaded:");
        println!("  prom_port: {}", prom_port);
        println!("  enable_prom: {}", enable_prom);
        println!("  otlp_endpoint: {}", otlp_endpoint);
        println!("  ups_name: {}", ups_name);
        println!("  ups_label: {}", ups_label);
        println!("  kwh_rate: {}", kwh_rate);
        println!("  interval: {}", interval);
        println!("  debug: {}", debug);
    }

    // Start background OTLP push loop if endpoint is specified
    start_otlp_push_loop(
        otlp_endpoint,
        ups_name.clone(),
        ups_label.clone(),
        kwh_rate,
        interval,
        debug,
    );

    // Run Prometheus endpoint if enabled, otherwise just sleep main thread
    if enable_prom {
        run_prometheus_server_blocking(prom_port, ups_name, ups_label, kwh_rate, debug);
    } else {
        println!("Prometheus server is disabled. Running OTLP push loop only.");
        loop {
            thread::sleep(std::time::Duration::from_secs(3600));
        }
    }
}
