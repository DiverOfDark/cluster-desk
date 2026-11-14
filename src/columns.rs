//! Extra table columns people add from the table header's ⋮ menu: any field of the object
//! (a label, an annotation, `status/nodeInfo/osImage`, …), saved per kind in columns.json.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::{OnceLock, RwLock};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct ExtraColumn {
    pub title: String,
    /// JSON Pointer without the leading "/" ("metadata/labels/kubernetes.io~1arch").
    pub path: String,
}

/// Keyed by `Kind::resource()` ("nodes", "pods", "<plural>.<group>").
type Columns = HashMap<String, Vec<ExtraColumn>>;

fn file() -> std::path::PathBuf {
    crate::settings::config_dir().join("columns.json")
}

fn store() -> &'static RwLock<Columns> {
    static S: OnceLock<RwLock<Columns>> = OnceLock::new();
    S.get_or_init(|| {
        let c = std::fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        RwLock::new(c)
    })
}

pub fn get(kind: &str) -> Vec<ExtraColumn> {
    store().read().ok().and_then(|s| s.get(kind).cloned()).unwrap_or_default()
}

pub fn set(kind: &str, cols: Vec<ExtraColumn>) {
    let Ok(mut s) = store().write() else { return };
    if cols.is_empty() {
        s.remove(kind);
    } else {
        s.insert(kind.to_string(), cols);
    }
    let f = file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&f, serde_json::to_string_pretty(&*s).unwrap_or_default());
}

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn unescape(seg: &str) -> String {
    seg.replace("~1", "/").replace("~0", "~")
}

/// What a column shows for an object: strings as they are, other values as compact JSON.
pub fn value(obj: &Value, path: &str) -> String {
    let mut cur = obj;
    for seg in path.split('/') {
        let seg = unescape(seg);
        cur = match cur {
            Value::Object(m) => m.get(&seg).unwrap_or(&Value::Null),
            Value::Array(a) => seg.parse::<usize>().ok().and_then(|i| a.get(i)).unwrap_or(&Value::Null),
            _ => &Value::Null,
        };
    }
    match cur {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Header title for a path: its last part, and for label-like keys the part after the last "/"
/// ("status/nodeInfo/osImage" → "osImage", "metadata/labels/kubernetes.io~1arch" → "arch").
pub fn default_title(path: &str) -> String {
    let last = unescape(path.trim_end_matches('/').rsplit('/').next().unwrap_or(path));
    last.rsplit('/').next().unwrap_or(&last).to_string()
}

/// Every scalar field in `objs` with an example value, for picking a column.
pub fn field_paths<'a>(objs: impl Iterator<Item = &'a Value>) -> Vec<(String, String)> {
    const MAX: usize = 3000;
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    fn walk(v: &Value, path: &mut String, out: &mut BTreeMap<String, String>) {
        if out.len() >= MAX {
            return;
        }
        let child = |key: &str, v: &Value, path: &mut String, out: &mut BTreeMap<String, String>| {
            let len = path.len();
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(&escape(key));
            walk(v, path, out);
            path.truncate(len);
        };
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    // Bookkeeping nobody wants as a column.
                    if path == "metadata" && k == "managedFields" {
                        continue;
                    }
                    if path == "metadata/annotations" && k == "kubectl.kubernetes.io/last-applied-configuration" {
                        continue;
                    }
                    child(k, x, path, out);
                }
            }
            Value::Array(a) => {
                for (i, x) in a.iter().enumerate().take(20) {
                    child(&i.to_string(), x, path, out);
                }
            }
            Value::Null => {}
            Value::String(s) => {
                out.entry(path.clone()).or_insert_with(|| s.clone());
            }
            other => {
                out.entry(path.clone()).or_insert_with(|| other.to_string());
            }
        }
    }
    for o in objs {
        walk(o, &mut String::new(), &mut out);
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_label_keys_with_slashes() {
        let o = json!({"metadata": {"labels": {"kubernetes.io/arch": "amd64"}}, "status": {"nodeInfo": {"osImage": "Talos (v1.9.0)"}}});
        let paths = field_paths(std::iter::once(&o));
        let label = paths.iter().find(|p| p.1 == "amd64").unwrap();
        assert_eq!(label.0, "metadata/labels/kubernetes.io~1arch");
        assert_eq!(value(&o, &label.0), "amd64");
        assert_eq!(default_title(&label.0), "arch");
        assert_eq!(value(&o, "status/nodeInfo/osImage"), "Talos (v1.9.0)");
        assert_eq!(default_title("status/nodeInfo/osImage"), "osImage");
    }
}
