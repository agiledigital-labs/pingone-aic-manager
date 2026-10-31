//! Structural JSON comparison shared by the verticals that show a diff of a
//! document rather than of text (`managed diff`, `journey diff`). Each caller
//! names the two sides itself; this module only finds where they differ.

use serde_json::Value;

/// One place two values disagree: a JSON pointer, and each side's value there
/// (`None` where that side has no such key or index).
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub path: String,
    pub left: Option<Value>,
    pub right: Option<Value>,
}

fn pointer_segment(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// Every place `left` and `right` differ. Object keys compare as a set, in
/// sorted order; arrays by position, because order is meaning in the
/// documents this serves (`schema.order`, a journey's node list).
pub fn changes(left: &Value, right: &Value) -> Vec<Change> {
    let mut out = Vec::new();
    walk("", left, right, &mut out);
    out
}

fn walk(at: &str, left: &Value, right: &Value, out: &mut Vec<Change>) {
    if left == right {
        return;
    }
    let one_side = |path: String, left: Option<&Value>, right: Option<&Value>| Change {
        path,
        left: left.cloned(),
        right: right.cloned(),
    };
    match (left, right) {
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let here = format!("{at}/{}", pointer_segment(key));
                match (a.get(key), b.get(key)) {
                    (Some(x), Some(y)) => walk(&here, x, y, out),
                    (x, y) if x.is_some() || y.is_some() => out.push(one_side(here, x, y)),
                    _ => {}
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            for index in 0..a.len().max(b.len()) {
                let here = format!("{at}/{index}");
                match (a.get(index), b.get(index)) {
                    (Some(x), Some(y)) => walk(&here, x, y, out),
                    (x, y) => out.push(one_side(here, x, y)),
                }
            }
        }
        _ => out.push(Change {
            path: if at.is_empty() { "/".into() } else { at.into() },
            left: Some(left.clone()),
            right: Some(right.clone()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn positional_arrays_escaped_keys_and_one_sided_entries() {
        let a = json!({"order": ["a", "b"], "x/y": 1, "gone": true});
        let b = json!({"order": ["b", "a", "c"], "x/y": 2, "new": false});
        let found = changes(&a, &b);
        let paths: Vec<&str> = found.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["/gone", "/new", "/order/0", "/order/1", "/order/2", "/x~1y"]
        );
        assert_eq!(found[0].right, None);
        assert_eq!(found[1].left, None);
        assert_eq!(changes(&json!(1), &json!("1"))[0].path, "/");
        assert!(changes(&a, &a).is_empty());
    }
}
