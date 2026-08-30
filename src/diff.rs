use std::collections::BTreeSet;

use serde_json::Value;

pub fn json_diff(before: &Value, after: &Value) -> String {
    let mut lines = Vec::new();
    compare_json("$", before, after, &mut lines);
    if lines.is_empty() {
        "No differences".to_owned()
    } else {
        lines.join("\n")
    }
}

pub fn text_diff(before_label: &str, before: &str, after_label: &str, after: &str) -> String {
    let left: Vec<&str> = before.lines().collect();
    let right: Vec<&str> = after.lines().collect();
    let mut lengths = vec![vec![0usize; right.len() + 1]; left.len() + 1];
    for left_index in (0..left.len()).rev() {
        for right_index in (0..right.len()).rev() {
            lengths[left_index][right_index] = if left[left_index] == right[right_index] {
                lengths[left_index + 1][right_index + 1] + 1
            } else {
                lengths[left_index + 1][right_index].max(lengths[left_index][right_index + 1])
            };
        }
    }
    let mut output = vec![format!("--- {before_label}"), format!("+++ {after_label}")];
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() || right_index < right.len() {
        if left_index < left.len()
            && right_index < right.len()
            && left[left_index] == right[right_index]
        {
            output.push(format!(" {}", left[left_index]));
            left_index += 1;
            right_index += 1;
        } else if right_index < right.len()
            && (left_index == left.len()
                || lengths[left_index][right_index + 1] >= lengths[left_index + 1][right_index])
        {
            output.push(format!("+{}", right[right_index]));
            right_index += 1;
        } else {
            output.push(format!("-{}", left[left_index]));
            left_index += 1;
        }
    }
    output.join("\n")
}

pub fn display_content(content_type: &str, content: &Value) -> Option<String> {
    if content_type == "application/json" || !content.is_string() {
        serde_json::to_string_pretty(content).ok()
    } else if content_type.starts_with("text/")
        || matches!(content_type, "application/yaml" | "application/x-yaml")
    {
        content.as_str().map(str::to_owned)
    } else {
        None
    }
}

fn compare_json(path: &str, before: &Value, after: &Value, lines: &mut Vec<String>) {
    match (before, after) {
        (Value::Object(left), Value::Object(right)) => {
            let keys: BTreeSet<_> = left.keys().chain(right.keys()).collect();
            for key in keys {
                let child_path = format!("{path}.{key}");
                match (left.get(key), right.get(key)) {
                    (Some(a), Some(b)) => compare_json(&child_path, a, b, lines),
                    (Some(a), None) => lines.push(format!("- {child_path} = {}", compact(a))),
                    (None, Some(b)) => lines.push(format!("+ {child_path} = {}", compact(b))),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            for index in 0..left.len().max(right.len()) {
                let child_path = format!("{path}[{index}]");
                match (left.get(index), right.get(index)) {
                    (Some(a), Some(b)) => compare_json(&child_path, a, b, lines),
                    (Some(a), None) => lines.push(format!("- {child_path} = {}", compact(a))),
                    (None, Some(b)) => lines.push(format!("+ {child_path} = {}", compact(b))),
                    (None, None) => {}
                }
            }
        }
        _ if before != after => lines.push(format!(
            "~ {path}: {} -> {}",
            compact(before),
            compact(after)
        )),
        _ => {}
    }
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<invalid JSON>".to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn reports_structural_json_changes() {
        let output = json_diff(&json!({"a": 1, "gone": true}), &json!({"a": 2, "new": []}));
        assert!(output.contains("~ $.a: 1 -> 2"));
        assert!(output.contains("- $.gone = true"));
        assert!(output.contains("+ $.new = []"));
    }

    #[test]
    fn produces_unified_text_lines() {
        assert_eq!(
            text_diff("v1", "a\nb", "v2", "a\nc"),
            "--- v1\n+++ v2\n a\n+c\n-b"
        );
    }
}
