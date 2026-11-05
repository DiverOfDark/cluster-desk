//! Small helpers: time parsing/formatting, Kubernetes quantities, JSON access.

use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Parse "2024-03-01T12:34:56Z" (fractional seconds and offsets tolerated).
pub fn parse_time(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |a: usize, l: usize| s.get(a..a + l)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 2)?, n(8, 2)?, n(11, 2)?, n(14, 2)?, n(17, 2)?);
    let mut t = days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se;
    // timezone offset like +02:00
    if let Some(pos) = s[19..].find(['+', '-']) {
        let off = &s[19 + pos..];
        if off.len() >= 6 {
            let sign = if off.starts_with('-') { -1 } else { 1 };
            let oh = off[1..3].parse::<i64>().unwrap_or(0);
            let om = off[4..6].parse::<i64>().unwrap_or(0);
            t -= sign * (oh * 3600 + om * 60);
        }
    }
    Some(t)
}

pub fn format_time(t: i64) -> String {
    let (y, m, d) = civil_from_days(t.div_euclid(86400));
    let s = t.rem_euclid(86400);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        s / 3600,
        (s / 60) % 60,
        s % 60
    )
}

/// Compact age like "5d", "3h", "12m", "40s" (two units for days/hours like kubectl).
pub fn age(created: i64, now: i64) -> String {
    if created <= 0 {
        return "-".into();
    }
    let s = (now - created).max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86400 {
        let h = s / 3600;
        let m = (s % 3600) / 60;
        if h < 10 && m > 0 { format!("{h}h{m}m") } else { format!("{h}h") }
    } else {
        let d = s / 86400;
        let h = (s % 86400) / 3600;
        if d < 10 && h > 0 { format!("{d}d{h}h") } else { format!("{d}d") }
    }
}

/// Parse a Kubernetes quantity into a plain number (bytes or cores).
pub fn quantity(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return 0.0;
    }
    let suffixes: [(&str, f64); 13] = [
        ("Ki", 1024.0),
        ("Mi", 1024.0f64.powi(2)),
        ("Gi", 1024.0f64.powi(3)),
        ("Ti", 1024.0f64.powi(4)),
        ("Pi", 1024.0f64.powi(5)),
        ("Ei", 1024.0f64.powi(6)),
        ("n", 1e-9),
        ("u", 1e-6),
        ("m", 1e-3),
        ("k", 1e3),
        ("M", 1e6),
        ("G", 1e9),
        ("T", 1e12),
    ];
    for (suf, mul) in suffixes {
        if let Some(num) = s.strip_suffix(suf) {
            return num.parse::<f64>().unwrap_or(0.0) * mul;
        }
    }
    s.parse::<f64>().unwrap_or(0.0)
}

pub fn fmt_bytes(b: f64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = b;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{v:.0}{}", units[i])
    } else {
        format!("{v:.1}{}", units[i])
    }
}

pub fn fmt_cores(c: f64) -> String {
    if c == 0.0 {
        "0".into()
    } else if c < 1.0 {
        format!("{:.0}m", c * 1000.0)
    } else if (c - c.round()).abs() < 0.001 {
        format!("{}", c.round() as i64)
    } else {
        format!("{c:.2}")
    }
}

/// Walk a JSON value by a slash-separated path ("metadata/name").
pub fn get<'a>(v: &'a Value, path: &str) -> &'a Value {
    let mut cur = v;
    for p in path.split('/') {
        cur = match cur {
            Value::Object(m) => m.get(p).unwrap_or(&Value::Null),
            Value::Array(a) => p.parse::<usize>().ok().and_then(|i| a.get(i)).unwrap_or(&Value::Null),
            _ => &Value::Null,
        };
    }
    cur
}

pub fn s(v: &Value, path: &str) -> String {
    match get(v, path) {
        Value::String(x) => x.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

pub fn i(v: &Value, path: &str) -> i64 {
    match get(v, path) {
        Value::Number(n) => n.as_i64().unwrap_or(0),
        Value::String(x) => x.parse().unwrap_or(0),
        _ => 0,
    }
}

pub fn arr<'a>(v: &'a Value, path: &str) -> &'a [Value] {
    match get(v, path) {
        Value::Array(a) => a.as_slice(),
        _ => &[],
    }
}

pub fn map_pairs(v: &Value, path: &str) -> Vec<(String, String)> {
    match get(v, path) {
        Value::Object(m) => m
            .iter()
            .map(|(k, v)| {
                let val = match v {
                    Value::String(s) => s.clone(),
                    o => o.to_string(),
                };
                (k.clone(), val)
            })
            .collect(),
        _ => vec![],
    }
}

/// YAML for display/editing, without the noisy managedFields.
pub fn to_yaml(v: &Value) -> String {
    let mut v = v.clone();
    if let Some(m) = v.get_mut("metadata").and_then(|m| m.as_object_mut()) {
        m.remove("managedFields");
    }
    serde_yaml::to_string(&v).unwrap_or_else(|e| format!("# could not render: {e}"))
}

pub fn from_yaml(text: &str) -> Result<Value, String> {
    serde_yaml::from_str::<Value>(text).map_err(|e| e.to_string())
}

/// One rule path of an Ingress, as the original's computeRuleDeclarations builds it.
pub struct IngressRoute {
    pub host: String,
    pub pathname: String,
    pub url: String,
    pub service: String,
    /// False when the host contains "*" (no host means "*"): shown as text, not a link.
    pub link: bool,
}

/// computeRuleDeclarations: protocol is https when spec.tls has any entry, otherwise http;
/// host defaults to "*", path to "/"; url = protocol://host + path; backend "name:port".
pub fn ingress_routes(v: &Value) -> Vec<IngressRoute> {
    let https = !arr(v, "spec/tls").is_empty();
    let protocol = if https { "https" } else { "http" };
    let mut out = vec![];
    for r in arr(v, "spec/rules") {
        let mut host = s(r, "host");
        if host.is_empty() {
            host = "*".into();
        }
        for p in arr(r, "http/paths") {
            let mut path = s(p, "path");
            if path.is_empty() {
                path = "/".into();
            }
            // getBackendServiceNamePort: "name:" + port.number ?? port.name
            let port = {
                let n = s(p, "backend/service/port/number");
                if n.is_empty() { s(p, "backend/service/port/name") } else { n }
            };
            let service = if s(p, "backend/service/name").is_empty() {
                "<unknown>".to_string()
            } else {
                format!("{}:{}", s(p, "backend/service/name"), port)
            };
            out.push(IngressRoute {
                link: !host.contains('*'),
                url: format!("{protocol}://{host}{path}"),
                host: host.clone(),
                pathname: path,
                service,
            });
        }
    }
    out
}

/// Addresses that can be opened in a browser (link routes only), without duplicates.
pub fn ingress_urls(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for r in ingress_routes(v) {
        if r.link && !out.contains(&r.url) {
            out.push(r.url);
        }
    }
    out
}

/// getPorts: 80 when any rule has http paths (or the default backend port when there are no
/// rules), plus 443 when there is TLS.
pub fn ingress_ports(v: &Value) -> String {
    let mut ports: Vec<String> = vec![];
    let rules = arr(v, "spec/rules");
    if !rules.is_empty() {
        if rules.iter().any(|r| !get(r, "http").is_null()) {
            ports.push("80".into());
        }
    } else {
        let p = s(v, "spec/defaultBackend/service/port/number");
        if !p.is_empty() {
            ports.push(p);
        }
    }
    if !arr(v, "spec/tls").is_empty() {
        ports.push("443".into());
    }
    ports.join(", ")
}

/// getLoadBalancers: hostname, or ip.
pub fn ingress_load_balancers(v: &Value) -> Vec<(String, String)> {
    arr(v, "status/loadBalancer/ingress").iter().map(|x| (s(x, "hostname"), s(x, "ip"))).collect()
}

/// Open an address in the person's default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    r.map(|_| ()).map_err(|e| format!("Could not open the browser: {e}"))
}
