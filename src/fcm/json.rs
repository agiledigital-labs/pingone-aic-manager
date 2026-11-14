//! `deepSort` followed by Node's `JSON.stringify(value, null, 2)`.
use serde_json::Value;

use crate::Result;

pub(super) fn bytes(value: &Value) -> Result<Vec<u8>> {
    let mut output = String::new();
    write(value, 0, &mut output)?;
    Ok(output.into_bytes())
}

fn index(key: &str) -> Option<u32> {
    key.parse::<u32>()
        .ok()
        .filter(|number| *number != u32::MAX && number.to_string() == key)
}

fn write(value: &Value, depth: usize, output: &mut String) -> Result<()> {
    match value {
        Value::Number(number) => {
            // JSON.parse represents every number as an IEEE-754 double,
            // including integer literals outside JS's exact integer range.
            let number = number.as_f64().expect("JSON number is finite");
            output.push_str(ryu_js::Buffer::new().format_finite(number));
        }
        Value::Array(values) if !values.is_empty() => {
            output.push_str("[\n");
            for (position, value) in values.iter().enumerate() {
                if position != 0 {
                    output.push_str(",\n");
                }
                output.push_str(&"  ".repeat(depth + 1));
                write(value, depth + 1, output)?;
            }
            output.push('\n');
            output.push_str(&"  ".repeat(depth));
            output.push(']');
        }
        Value::Object(values) => {
            // deepSort assigns to a plain JS object: __proto__ invokes its
            // prototype setter and does not become an own enumerable key.
            let mut entries: Vec<_> = values
                .iter()
                .filter(|(key, _)| key.as_str() != "__proto__")
                .collect();
            entries.sort_by(|(left, _), (right, _)| match (index(left), index(right)) {
                (Some(left), Some(right)) => left.cmp(&right),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => left.encode_utf16().cmp(right.encode_utf16()),
            });
            if entries.is_empty() {
                output.push_str("{}");
                return Ok(());
            }
            output.push_str("{\n");
            for (position, (key, value)) in entries.into_iter().enumerate() {
                if position != 0 {
                    output.push_str(",\n");
                }
                output.push_str(&"  ".repeat(depth + 1));
                output.push_str(&serde_json::to_string(key)?);
                output.push_str(": ");
                write(value, depth + 1, output)?;
            }
            output.push('\n');
            output.push_str(&"  ".repeat(depth));
            output.push('}');
        }
        // serde_json uses the same JSON string escapes as Node 24, leaving
        // U+2028 and U+2029 as UTF-8. Empty arrays and scalar literals agree.
        _ => output.push_str(&serde_json::to_string(value)?),
    }
    Ok(())
}
