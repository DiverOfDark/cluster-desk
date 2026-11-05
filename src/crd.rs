//! Custom resource types discovered from the cluster's CustomResourceDefinitions.
//! Each definition becomes a `Kind::Custom(i)`, so lists, details, YAML editing and delete work
//! for it like for built-in kinds. Rules follow the original (see MECHANICS.md, "Custom resources").

use crate::util::{arr, get, s};
use serde_json::Value;
use std::sync::{OnceLock, RwLock};

pub struct CrdInfo {
    /// kubectl resource name: "<plural>.<group>"
    pub key: &'static str,
    pub group: &'static str,
    pub kind: &'static str,
    pub plural: &'static str,
    /// Sidebar/list title: the kind split at case changes ("ServiceMonitor" → "Service Monitor").
    pub title: &'static str,
    pub namespaced: bool,
    pub columns: &'static [(&'static str, f32)],
    /// Extra printer columns (jsonPath), same order as `columns` after Name/Namespace.
    pub printer: Vec<String>,
    signature: String,
}

fn reg() -> &'static RwLock<Vec<&'static CrdInfo>> {
    static R: OnceLock<RwLock<Vec<&'static CrdInfo>>> = OnceLock::new();
    R.get_or_init(|| RwLock::new(Vec::new()))
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

pub fn info(i: u16) -> &'static CrdInfo {
    reg().read().unwrap()[i as usize]
}

/// All registered custom kinds, sorted by group then plural name (as the original's sidebar).
pub fn all() -> Vec<(u16, &'static CrdInfo)> {
    let r = reg().read().unwrap();
    let mut v: Vec<(u16, &'static CrdInfo)> = r.iter().enumerate().map(|(i, c)| (i as u16, *c)).collect();
    v.sort_by(|a, b| (a.1.group, a.1.plural).cmp(&(b.1.group, b.1.plural)));
    v
}

/// formatResourceKind: split on /(?<=[a-z])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])/ and join with spaces.
pub fn format_kind(kind: &str) -> String {
    let c: Vec<char> = kind.chars().collect();
    let mut out = String::new();
    for i in 0..c.len() {
        if i > 0 {
            let split = (c[i - 1].is_lowercase() && c[i].is_uppercase())
                || (c[i - 1].is_uppercase() && c[i].is_uppercase() && c.get(i + 1).map(|n| n.is_lowercase()).unwrap_or(false));
            if split {
                out.push(' ');
            }
        }
        out.push(c[i]);
    }
    out
}

/// lodash startCase: words split at separators and case changes, each word's first letter upper.
pub fn start_case(name: &str) -> String {
    let spaced = format_kind(name);
    spaced
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// getPreferredVersion (apiextensions v1): the storage version if served, else the first served,
/// else the storage version.
fn preferred_version(v: &Value) -> Option<&Value> {
    let versions = arr(v, "spec/versions");
    let storage = versions.iter().find(|x| get(x, "storage").as_bool().unwrap_or(false));
    if let Some(sv) = storage {
        if get(sv, "served").as_bool().unwrap_or(false) {
            return Some(sv);
        }
    }
    versions.iter().find(|x| get(x, "served").as_bool().unwrap_or(false)).or(storage)
}

/// Register (or refresh) a CustomResourceDefinition; returns its kind.
pub fn register(v: &Value) -> Option<crate::model::Kind> {
    let group = s(v, "spec/group");
    let plural = s(v, "spec/names/plural");
    let kind = s(v, "spec/names/kind");
    if group.is_empty() || plural.is_empty() {
        return None;
    }
    let key = format!("{plural}.{group}");
    let namespaced = s(v, "spec/scope") != "Cluster";
    // getPrinterColumns(false): drop "age" and columns with a priority.
    let mut printer: Vec<(String, String)> = vec![];
    if let Some(pv) = preferred_version(v) {
        for c in arr(pv, "additionalPrinterColumns") {
            let name = s(c, "name");
            if name.to_lowercase() == "age" || get(c, "priority").as_i64().unwrap_or(0) != 0 {
                continue;
            }
            printer.push((name, s(c, "jsonPath")));
        }
    }
    let signature = format!("{namespaced}|{kind}|{printer:?}");
    let mut r = reg().write().unwrap();
    if let Some(pos) = r.iter().position(|c| c.key == key) {
        if r[pos].signature == signature {
            return Some(crate::model::Kind::Custom(pos as u16));
        }
    }
    let mut cols: Vec<(&'static str, f32)> = vec![("Name", 2.4)];
    if namespaced {
        cols.push(("Namespace", 1.3));
    }
    for (n, _) in &printer {
        cols.push((leak(start_case(n)), 1.1));
    }
    cols.push(("Age", 0.6));
    let info: &'static CrdInfo = Box::leak(Box::new(CrdInfo {
        key: leak(key.clone()),
        group: leak(group),
        kind: leak(kind.clone()),
        plural: leak(plural),
        title: leak(format_kind(&kind)),
        namespaced,
        columns: Box::leak(cols.into_boxed_slice()),
        printer: printer.into_iter().map(|p| p.1).collect(),
        signature,
    }));
    let idx = match r.iter().position(|c| c.key == key) {
        Some(pos) => {
            r[pos] = info;
            pos
        }
        None => {
            r.push(info);
            r.len() - 1
        }
    };
    Some(crate::model::Kind::Custom(idx as u16))
}

// ------------------------------------------------------------------ kubectl-style JSONPath

/// safeJSONPathValue + formatJSONValue: evaluate a kubectl JSONPath such as
/// `.status.conditions[?(@.type=="Ready")].status` and format the result
/// (arrays joined with ", ", objects as JSON, nothing → "").
pub fn json_path_text(obj: &Value, path: &str) -> String {
    let (vals, many) = json_path(obj, path);
    let vals: Vec<&Value> = if many { vals } else { vals.into_iter().take(1).collect() };
    let parts: Vec<String> = vals.iter().map(|v| format_value(v)).collect();
    parts.join(", ")
}

/// Raw JSONPath results (first match unless the path has a wildcard), for the details view.
pub fn json_path_values(obj: &Value, path: &str) -> Vec<Value> {
    let (vals, many) = json_path(obj, path);
    let vals: Vec<&Value> = if many { vals } else { vals.into_iter().take(1).collect() };
    vals.into_iter().cloned().collect()
}

fn format_value(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(format_value).collect::<Vec<_>>().join(", "),
        Value::Object(_) => v.to_string(),
        other => other.to_string(),
    }
}

enum Step {
    Key(String),
    Index(usize),
    All,
    Filter(String, String),
}

fn parse_path(path: &str) -> Vec<Step> {
    let p = path.trim().trim_start_matches('{').trim_end_matches('}').trim_start_matches('$');
    let c: Vec<char> = p.chars().collect();
    let mut steps = vec![];
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '.' => i += 1,
            '[' => {
                let end = c[i..].iter().position(|&x| x == ']').map(|e| i + e).unwrap_or(c.len());
                let inner: String = c[i + 1..end].iter().collect();
                let inner = inner.trim();
                if inner == "*" {
                    steps.push(Step::All);
                } else if let Ok(n) = inner.parse::<usize>() {
                    steps.push(Step::Index(n));
                } else if inner.starts_with("?(") {
                    // ?(@.key=="value")
                    let body = inner.trim_start_matches("?(").trim_end_matches(')');
                    if let Some((l, r)) = body.split_once("==") {
                        let key = l.trim().trim_start_matches("@.").to_string();
                        let val = r.trim().trim_matches(|ch| ch == '"' || ch == '\'').to_string();
                        steps.push(Step::Filter(key, val));
                    }
                } else {
                    steps.push(Step::Key(inner.trim_matches(|ch| ch == '"' || ch == '\'').to_string()));
                }
                i = end + 1;
            }
            _ => {
                let mut key = String::new();
                while i < c.len() && c[i] != '.' && c[i] != '[' {
                    if c[i] == '\\' && i + 1 < c.len() {
                        key.push(c[i + 1]);
                        i += 2;
                        continue;
                    }
                    key.push(c[i]);
                    i += 1;
                }
                steps.push(Step::Key(key));
            }
        }
    }
    steps
}

/// Returns the matches and whether the path is a slice/wildcard (then all matches are shown).
fn json_path<'a>(obj: &'a Value, path: &str) -> (Vec<&'a Value>, bool) {
    let steps = parse_path(path);
    let many = steps.iter().any(|s| matches!(s, Step::All));
    let mut cur: Vec<&Value> = vec![obj];
    for st in &steps {
        let mut next = vec![];
        for v in cur {
            match st {
                Step::Key(k) => {
                    if let Some(x) = v.get(k.as_str()) {
                        next.push(x);
                    }
                }
                Step::Index(n) => {
                    if let Some(x) = v.get(*n) {
                        next.push(x);
                    }
                }
                Step::All => match v {
                    Value::Array(a) => next.extend(a.iter()),
                    Value::Object(o) => next.extend(o.values()),
                    _ => {}
                },
                Step::Filter(k, val) => {
                    if let Value::Array(a) = v {
                        for x in a {
                            let got = k.split('.').fold(Some(x), |acc, part| acc.and_then(|y| y.get(part)));
                            if got.map(format_value).as_deref() == Some(val.as_str()) {
                                next.push(x);
                            }
                        }
                    }
                }
            }
        }
        cur = next;
    }
    (cur, many)
}
