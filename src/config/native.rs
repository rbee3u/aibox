//! Native JSON/TOML Config mechanics and direct file editing facade.

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};
use toml_edit::{DocumentMut, Item, Table, TableLike};

pub(crate) use super::editing::{
    config_file_warnings, diagnose_config_file, inspect_named_codex_auth, read_config_file_target,
    save_config_file_target, visual_config_state,
};

pub(super) fn parse_json_object(content: &str, label: &str) -> Result<Map<String, Value>> {
    let value = serde_json::from_str::<Value>(content).with_context(|| format!("parse {label}"))?;
    value
        .as_object()
        .cloned()
        .with_context(|| format!("{label} must be a JSON object"))
}

pub(super) fn value_at_path<'a>(
    object: &'a Map<String, Value>,
    path: &[&str],
) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let value = object.get(*first)?;
    if rest.is_empty() {
        Some(value)
    } else {
        value
            .as_object()
            .and_then(|child| value_at_path(child, rest))
    }
}

pub(super) fn set_json_path(object: &mut Map<String, Value>, path: &[&str], value: Value) -> bool {
    let Some((first, rest)) = path.split_first() else {
        return false;
    };
    if rest.is_empty() {
        if object.get(*first) == Some(&value) {
            return false;
        }
        object.insert((*first).to_string(), value);
        return true;
    }

    let mut changed = !matches!(object.get(*first), Some(Value::Object(_)));
    if changed {
        object.insert((*first).to_string(), Value::Object(Map::new()));
    }
    let child = object
        .get_mut(*first)
        .and_then(Value::as_object_mut)
        .expect("object inserted above");
    changed |= set_json_path(child, rest, value);
    changed
}

pub(super) fn remove_json_path(object: &mut Map<String, Value>, path: &[&str]) -> bool {
    let Some((first, rest)) = path.split_first() else {
        return false;
    };
    if rest.is_empty() {
        return object.remove(*first).is_some();
    }
    let Some(existing) = object.get_mut(*first) else {
        return false;
    };
    if !existing.is_object() {
        object.remove(*first);
        return true;
    }
    let child = existing.as_object_mut().expect("object checked above");
    let changed = remove_json_path(child, rest);
    if child.is_empty() {
        object.remove(*first);
        true
    } else {
        changed
    }
}

pub(super) fn set_codex_path(
    document: &mut DocumentMut,
    path: &[&str],
    value: &Value,
) -> Result<bool> {
    if path.len() == 1 {
        return set_toml_item(document.as_table_mut(), path[0], value);
    }
    debug_assert_eq!(&path[..2], ["model_providers", "custom"]);
    let mut changed = ensure_toml_table(document.as_table_mut(), "model_providers", true);
    let providers = document
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
        .context("model_providers must be a table")?;
    changed |= ensure_toml_table(providers, "custom", false);
    let custom = providers
        .get_mut("custom")
        .and_then(Item::as_table_like_mut)
        .context("model_providers.custom must be a table")?;
    changed |= set_toml_item(custom, path[2], value)?;
    Ok(changed)
}

fn set_toml_item(table: &mut dyn TableLike, key: &str, value: &Value) -> Result<bool> {
    if table
        .get(key)
        .is_some_and(|item| toml_item_matches(item, value))
    {
        return Ok(false);
    }
    let mut item = match value {
        Value::String(value) => toml_edit::value(value.clone()),
        Value::Bool(value) => toml_edit::value(*value),
        _ => bail!("Config Field has an unsupported TOML value type"),
    };
    if let Some(existing) = table.get_mut(key) {
        if let (Some(previous), Some(replacement)) = (existing.as_value(), item.as_value_mut()) {
            *replacement.decor_mut() = previous.decor().clone();
        }
        *existing = item;
    } else {
        table.insert(key, item);
    }
    Ok(true)
}

fn toml_item_matches(item: &Item, value: &Value) -> bool {
    let Some(item) = item.as_value() else {
        return false;
    };
    match value {
        Value::String(value) => item.as_str() == Some(value),
        Value::Bool(value) => item.as_bool() == Some(*value),
        _ => false,
    }
}

fn ensure_toml_table(table: &mut dyn TableLike, key: &str, implicit: bool) -> bool {
    if table.get(key).and_then(Item::as_table_like).is_some() {
        return false;
    }
    let mut child = Table::new();
    child.set_implicit(implicit);
    table.insert(key, Item::Table(child));
    true
}

pub(super) fn remove_codex_path(document: &mut DocumentMut, path: &[&str]) -> bool {
    if path.len() == 1 {
        return document.as_table_mut().remove(path[0]).is_some();
    }
    debug_assert_eq!(&path[..2], ["model_providers", "custom"]);

    if document.get("model_providers").is_some()
        && document
            .get("model_providers")
            .and_then(Item::as_table_like)
            .is_none()
    {
        document.as_table_mut().remove("model_providers");
        return true;
    }

    let mut changed = false;
    let mut remove_providers = false;
    if let Some(providers) = document
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
    {
        if providers.get("custom").is_some()
            && providers
                .get("custom")
                .and_then(Item::as_table_like)
                .is_none()
        {
            providers.remove("custom");
            changed = true;
        }
        let mut remove_custom = false;
        if let Some(custom) = providers
            .get_mut("custom")
            .and_then(Item::as_table_like_mut)
        {
            changed |= custom.remove(path[2]).is_some();
            remove_custom = custom.iter().next().is_none();
        }
        if remove_custom {
            providers.remove("custom");
            changed = true;
        }
        remove_providers = providers.iter().next().is_none();
    }
    if remove_providers {
        document.as_table_mut().remove("model_providers");
        changed = true;
    }
    changed
}

pub(super) fn remove_codex_provider(document: &mut DocumentMut) -> bool {
    let mut changed = document.as_table_mut().remove("model_provider").is_some();
    changed |= document.as_table_mut().remove("model_providers").is_some();
    changed
}
