use anyhow::{bail, Result};
use serde_json::Value as JsonValue;
use toml_edit::{DocumentMut, Item, TableLike};

pub fn merge_json(base: &mut JsonValue, overlay: JsonValue) {
    match (base, overlay) {
        (JsonValue::Object(base_map), JsonValue::Object(overlay_map)) => {
            for (key, overlay_value) in overlay_map {
                match base_map.get_mut(&key) {
                    Some(base_value) => merge_json(base_value, overlay_value),
                    None => {
                        base_map.insert(key, overlay_value);
                    }
                }
            }
        }
        (base_value, overlay_value) => {
            *base_value = overlay_value;
        }
    }
}

pub fn merge_toml_strings(base: &str, overlay: &str) -> Result<String> {
    let mut base_doc = parse_toml_or_empty(base)?;
    let mut overlay_doc = parse_toml_or_empty(overlay)?;
    let remove_paths = extract_toml_remove_paths(&overlay_doc)?;
    base_doc.as_table_mut().remove("aibox");
    overlay_doc.as_table_mut().remove("aibox");
    merge_toml_table_like(base_doc.as_table_mut(), overlay_doc.as_table());
    for path in remove_paths {
        remove_toml_path(base_doc.as_table_mut(), &path)?;
    }
    base_doc.as_table_mut().remove("aibox");
    Ok(base_doc.to_string())
}

pub fn merge_json_with_apply_metadata(base: &mut JsonValue, mut overlay: JsonValue) -> Result<()> {
    let remove_paths = extract_json_remove_paths(&overlay)?;
    remove_reserved_json_metadata(base);
    remove_reserved_json_metadata(&mut overlay);
    merge_json(base, overlay);
    for path in remove_paths {
        remove_json_path(base, &path)?;
    }
    remove_reserved_json_metadata(base);
    Ok(())
}

pub fn parse_json_or_empty_object(content: &str) -> Result<JsonValue> {
    if content.trim().is_empty() {
        Ok(JsonValue::Object(Default::default()))
    } else {
        Ok(serde_json::from_str(content)?)
    }
}

fn parse_toml_or_empty(content: &str) -> Result<DocumentMut> {
    if content.trim().is_empty() {
        Ok(DocumentMut::new())
    } else {
        Ok(content.parse::<DocumentMut>()?)
    }
}

fn extract_toml_remove_paths(doc: &DocumentMut) -> Result<Vec<String>> {
    let Some(aibox) = doc.as_table().get("aibox") else {
        return Ok(Vec::new());
    };
    let Some(aibox_table) = aibox.as_table_like() else {
        bail!("aibox metadata must be a table");
    };
    let Some(apply) = aibox_table.get("apply") else {
        return Ok(Vec::new());
    };
    let Some(apply_table) = apply.as_table_like() else {
        bail!("aibox.apply metadata must be a table");
    };
    let Some(remove) = apply_table.get("remove") else {
        return Ok(Vec::new());
    };
    let Some(paths) = remove.as_array() else {
        bail!("aibox.apply.remove must be an array of strings");
    };

    paths
        .iter()
        .map(|value| {
            let Some(path) = value.as_str() else {
                bail!("aibox.apply.remove must be an array of strings");
            };
            validate_remove_path(path)?;
            Ok(path.to_string())
        })
        .collect()
}

fn extract_json_remove_paths(value: &JsonValue) -> Result<Vec<String>> {
    let Some(root) = value.as_object() else {
        return Ok(Vec::new());
    };
    let Some(aibox) = root.get("aibox") else {
        return Ok(Vec::new());
    };
    let Some(aibox_object) = aibox.as_object() else {
        bail!("aibox metadata must be a JSON object");
    };
    let Some(apply) = aibox_object.get("apply") else {
        return Ok(Vec::new());
    };
    let Some(apply_object) = apply.as_object() else {
        bail!("aibox.apply metadata must be a JSON object");
    };
    let Some(remove) = apply_object.get("remove") else {
        return Ok(Vec::new());
    };
    let Some(paths) = remove.as_array() else {
        bail!("aibox.apply.remove must be an array of strings");
    };

    paths
        .iter()
        .map(|value| {
            let Some(path) = value.as_str() else {
                bail!("aibox.apply.remove must be an array of strings");
            };
            validate_remove_path(path)?;
            Ok(path.to_string())
        })
        .collect()
}

fn validate_remove_path(path: &str) -> Result<()> {
    path_segments(path).map(|_| ())
}

fn path_segments(path: &str) -> Result<Vec<&str>> {
    let segments: Vec<_> = path.split('.').collect();
    if segments.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
        bail!("aibox.apply.remove path must be a non-empty dotted key path: {path:?}");
    }
    Ok(segments)
}

fn remove_toml_path(table: &mut dyn TableLike, path: &str) -> Result<()> {
    let segments = path_segments(path)?;
    remove_toml_segments(table, &segments);
    Ok(())
}

fn remove_toml_segments(table: &mut dyn TableLike, segments: &[&str]) {
    if segments.len() == 1 {
        table.remove(segments[0]);
        return;
    }

    let Some(item) = table.get_mut(segments[0]) else {
        return;
    };
    let Some(next_table) = item.as_table_like_mut() else {
        return;
    };
    remove_toml_segments(next_table, &segments[1..]);
}

fn remove_json_path(value: &mut JsonValue, path: &str) -> Result<()> {
    let segments = path_segments(path)?;
    remove_json_segments(value, &segments);
    Ok(())
}

fn remove_json_segments(value: &mut JsonValue, segments: &[&str]) {
    let Some((first, rest)) = segments.split_first() else {
        return;
    };
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if rest.is_empty() {
        object.remove(*first);
        return;
    }
    let Some(next_value) = object.get_mut(*first) else {
        return;
    };
    remove_json_segments(next_value, rest);
}

fn remove_reserved_json_metadata(value: &mut JsonValue) {
    if let Some(object) = value.as_object_mut() {
        object.remove("aibox");
    }
}

fn merge_toml_table_like(base: &mut dyn TableLike, overlay: &dyn TableLike) {
    for (key, overlay_item) in overlay.iter() {
        match base.get_mut(key) {
            Some(base_item) => merge_toml_items(base_item, overlay_item),
            None => {
                base.insert(key, overlay_item.clone());
            }
        }
    }
}

fn merge_toml_items(base: &mut Item, overlay: &Item) {
    match (base.as_table_like_mut(), overlay.as_table_like()) {
        (Some(base_table), Some(overlay_table)) => {
            merge_toml_table_like(base_table, overlay_table);
        }
        _ => {
            *base = overlay.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_merge_recurses_objects_and_replaces_arrays() {
        let mut base = json!({
            "model": "a",
            "nested": {
                "keep": true,
                "replace": ["old"]
            }
        });
        let overlay = json!({
            "nested": {
                "replace": ["new"],
                "add": 1
            }
        });

        merge_json(&mut base, overlay);

        assert_eq!(
            base,
            json!({
                "model": "a",
                "nested": {
                    "keep": true,
                    "replace": ["new"],
                    "add": 1
                }
            })
        );
    }

    #[test]
    fn toml_merge_recurses_tables_and_preserves_unmentioned_keys() {
        let base = r#"
model = "common"

[model_providers.openai]
base_url = "old"
legacy = true
"#;
        let overlay = r#"
model = "provider"

[model_providers.openai]
base_url = "new"
"#;

        let merged = merge_toml_strings(base, overlay).unwrap();

        assert!(merged.contains(r#"model = "provider""#));
        assert!(merged.contains(r#"base_url = "new""#));
        assert!(merged.contains("legacy = true"));
    }

    #[test]
    fn toml_apply_metadata_removes_paths_after_merge_and_is_stripped() {
        let base = r#"
model = "common"
model_provider = "custom"

[model_providers.custom]
base_url = "old"

[aibox.apply]
remove = ["stale"]
"#;
        let overlay = r#"
model = "provider"
model_provider = "custom"

[model_providers.custom]
base_url = "new"

[aibox.apply]
remove = ["model_provider", "model_providers.custom", "missing.path"]
"#;

        let merged = merge_toml_strings(base, overlay).unwrap();

        assert!(merged.contains(r#"model = "provider""#));
        assert!(!merged.contains("model_provider"));
        assert!(!merged.contains("[model_providers.custom]"));
        assert!(!merged.contains("[aibox"));
    }

    #[test]
    fn toml_apply_metadata_rejects_bad_remove_paths() {
        let merged = merge_toml_strings("", r#"[aibox.apply]"#).unwrap();
        assert!(!merged.contains("[aibox"));

        let err = merge_toml_strings("", "[aibox.apply]\nremove = [\"\"]").unwrap_err();
        assert!(err.to_string().contains("non-empty dotted key path"));

        let err = merge_toml_strings("", "[aibox.apply]\nremove = [\"foo..bar\"]").unwrap_err();
        assert!(err.to_string().contains("non-empty dotted key path"));
    }

    #[test]
    fn json_apply_metadata_removes_paths_after_merge_and_is_stripped() {
        let mut base = json!({
            "model": "common",
            "nested": {
                "keep": true,
                "drop": true
            },
            "aibox": {
                "apply": {
                    "remove": ["stale"]
                }
            }
        });
        let overlay = json!({
            "model": "provider",
            "nested": {
                "drop": false,
                "add": 1
            },
            "aibox": {
                "apply": {
                    "remove": ["nested.drop", "missing.path"]
                }
            }
        });

        merge_json_with_apply_metadata(&mut base, overlay).unwrap();

        assert_eq!(
            base,
            json!({
                "model": "provider",
                "nested": {
                    "keep": true,
                    "add": 1
                }
            })
        );
    }
}
