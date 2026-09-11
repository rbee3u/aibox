//! Read-only comparison of Last Application's source and the visible editor drafts.

use super::application::read_last_application;
use super::definition::NamedConfigDefinition;
use super::editing::read_config_file_target;
use super::visual::{render_visual_auth, render_visual_main};
use super::{ConfigEdit, ConfigFile, ConfigTarget, MAX_CONFIG_BYTES, NamedConfigName};
use crate::tenant::TenantAgent;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) struct ConfigComparisonDraft {
    pub(crate) file: ConfigFile,
    pub(crate) revision: String,
    pub(crate) original: Vec<u8>,
    pub(crate) edit: ConfigEdit,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ConfigComparison {
    pub(crate) source: String,
    pub(crate) incomplete: bool,
    pub(crate) files: Vec<ConfigComparisonFile>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ConfigComparisonFile {
    pub(crate) file: String,
    pub(crate) error: Option<String>,
    pub(crate) named: Option<ConfigComparisonSide>,
    pub(crate) current: Option<ConfigComparisonSide>,
    pub(crate) differences: Vec<ConfigDifference>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ConfigComparisonSide {
    pub(crate) revision: String,
    pub(crate) exists: bool,
    pub(crate) content: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ConfigDifference {
    pub(crate) path: Vec<String>,
    pub(crate) sensitive: bool,
    pub(crate) named_present: bool,
    pub(crate) current_present: bool,
    pub(crate) named_value: Option<Value>,
    pub(crate) current_value: Option<Value>,
    // UTF-16 offsets for TOML; the editor locates JSON paths in its syntax tree.
    pub(crate) named_range: Option<[usize; 2]>,
    pub(crate) current_range: Option<[usize; 2]>,
}

pub(crate) fn compare_configs(
    selected: &TenantAgent,
    target: &ConfigTarget,
    drafts: Vec<ConfigComparisonDraft>,
) -> Result<ConfigComparison> {
    let last = read_last_application(selected)?.context("No Last Application to compare")?;
    let source = NamedConfigName::parse(&last.applied)?;
    if target.named().is_some_and(|name| name != &source) {
        bail!("Only the Last Application source can be compared with Current Config");
    }
    let mut seen = BTreeSet::new();
    for draft in &drafts {
        if draft.original.len() as u64 > MAX_CONFIG_BYTES {
            bail!("Comparison snapshot exceeds size limit");
        }
        if !seen.insert(draft.file.as_str(selected.agent())) {
            bail!("Duplicate comparison file");
        }
    }
    let named_target = ConfigTarget::Named(source);
    let mut files = ConfigFile::all(selected.agent())
        .map(|file| {
            compare_file(
                selected,
                target,
                &named_target,
                file,
                drafts.iter().find(|draft| draft.file == file),
            )
            .unwrap_or_else(|error| ConfigComparisonFile {
                file: file.as_str(selected.agent()).into(),
                error: Some(format!("{error:#}")),
                named: None,
                current: None,
                differences: Vec::new(),
            })
        })
        .collect::<Vec<_>>();
    // Independent comparisons still obey the source's cross-file credential contract.
    // Attribute that failure to auth so an otherwise inspectable main draft stays useful.
    if selected.agent().native_auth_file().is_some()
        && files.iter().all(|file| file.error.is_none())
    {
        let main = &files[0]
            .named
            .as_ref()
            .expect("successful comparison has source")
            .content;
        let auth = &files[1]
            .named
            .as_ref()
            .expect("successful comparison has source")
            .content;
        if let Err(error) = NamedConfigDefinition::parse(selected.agent(), main, Some(auth)) {
            files[1].error = Some(format!("{error:#}"));
            files[1].differences.clear();
        }
    }
    Ok(ConfigComparison {
        source: last.applied,
        incomplete: files.iter().any(|file| file.error.is_some()),
        files,
    })
}

fn compare_file(
    selected: &TenantAgent,
    target: &ConfigTarget,
    named_target: &ConfigTarget,
    file: ConfigFile,
    draft: Option<&ConfigComparisonDraft>,
) -> Result<ConfigComparisonFile> {
    let named_snapshot = read_config_file_target(selected, named_target, file)?;
    if !named_snapshot.exists {
        bail!("Last Application source file is missing");
    }
    let current_snapshot = read_config_file_target(selected, &ConfigTarget::Current, file)?;
    let mut named = ConfigComparisonSide {
        revision: named_snapshot.revision,
        exists: named_snapshot.exists,
        content: String::from_utf8(named_snapshot.content)
            .context("Named Config is not valid UTF-8")?,
    };
    let mut current = ConfigComparisonSide {
        revision: current_snapshot.revision,
        exists: current_snapshot.exists,
        content: String::from_utf8(current_snapshot.content)
            .context("Current Config is not valid UTF-8")?,
    };
    if let Some(draft) = draft {
        let side = if target.is_current() {
            &mut current
        } else {
            &mut named
        };
        if side.revision != draft.revision {
            bail!("Configuration changed since it was opened. Refresh to compare.");
        }
        let content = match &draft.edit {
            ConfigEdit::Raw { content, .. } => {
                String::from_utf8(content.clone()).context("Draft is not valid UTF-8")?
            }
            ConfigEdit::VisualMain {
                options,
                custom_provider,
            } => {
                if target.is_current() || file != ConfigFile::Main {
                    bail!("Invalid Visual main comparison target");
                }
                let original =
                    std::str::from_utf8(&draft.original).context("Draft is not valid UTF-8")?;
                render_visual_main(
                    selected.agent(),
                    original,
                    options,
                    custom_provider.as_ref(),
                )?
            }
            ConfigEdit::VisualAuth(auth) => {
                if target.is_current() || file != ConfigFile::Auth {
                    bail!("Invalid Visual auth comparison target");
                }
                render_visual_auth(auth)?
            }
        };
        if content.len() as u64 > MAX_CONFIG_BYTES {
            bail!("Comparison draft exceeds size limit");
        }
        side.content = content;
    }
    let agent = selected.agent();
    NamedConfigDefinition::validate_file(agent, file.as_str(agent), &named.content)?;
    let (desired, actual) = if file == ConfigFile::Main {
        // Application handles omitted fields, blocking parents, and native empty tables.
        let projected =
            NamedConfigDefinition::project_main_file(agent, &named.content, &current.content)?;
        (
            Value::Object(agent.parse_main_config(&projected)?),
            Value::Object(agent.parse_main_config(&current.content)?),
        )
    } else {
        let desired: Value = serde_json::from_str(&named.content)?;
        let actual: Value = serde_json::from_str(&current.content)?;
        if !actual.is_object() {
            bail!("Current Config auth.json must be an object");
        }
        (desired, actual)
    };
    let mut differences = Vec::new();
    collect_differences(
        Some(&desired),
        Some(&actual),
        &mut Vec::new(),
        &mut differences,
    );
    for difference in &mut differences {
        difference.sensitive = file == ConfigFile::Auth
            || agent.main_config_fields().iter().any(|field| {
                field.sensitive
                    && field
                        .path
                        .iter()
                        .zip(&difference.path)
                        .all(|(expected, actual)| *expected == actual)
            });
        if file.as_str(agent).ends_with(".toml") {
            difference.named_range = toml_range(&named.content, &difference.path);
            difference.current_range = toml_range(&current.content, &difference.path);
        }
    }
    Ok(ConfigComparisonFile {
        file: file.as_str(agent).into(),
        error: None,
        named: Some(named),
        current: Some(current),
        differences,
    })
}

fn collect_differences(
    named: Option<&Value>,
    current: Option<&Value>,
    path: &mut Vec<String>,
    output: &mut Vec<ConfigDifference>,
) {
    if named == current {
        return;
    }
    if (named.is_some_and(Value::is_object)
        && (current.is_none() || current.is_some_and(Value::is_object)))
        || (current.is_some_and(Value::is_object) && named.is_none())
    {
        let empty = serde_json::Map::new();
        let a = named.and_then(Value::as_object).unwrap_or(&empty);
        let b = current.and_then(Value::as_object).unwrap_or(&empty);
        if a.is_empty() && b.is_empty() {
            output.push(ConfigDifference {
                path: path.clone(),
                sensitive: false,
                named_present: named.is_some(),
                current_present: current.is_some(),
                named_value: named.cloned(),
                current_value: current.cloned(),
                named_range: None,
                current_range: None,
            });
            return;
        }
        for key in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
            path.push(key.clone());
            collect_differences(a.get(key), b.get(key), path, output);
            path.pop();
        }
    } else {
        output.push(ConfigDifference {
            path: path.clone(),
            sensitive: false,
            named_present: named.is_some(),
            current_present: current.is_some(),
            named_value: named.cloned(),
            current_value: current.cloned(),
            named_range: None,
            current_range: None,
        });
    }
}

fn toml_range(text: &str, path: &[String]) -> Option<[usize; 2]> {
    let document = toml_edit::Document::parse(text).ok()?;
    let mut item = document.as_item();
    for key in path {
        item = match item.get(key) {
            Some(child) => child,
            None => {
                return if item.is_table() || item.is_inline_table() {
                    None
                } else {
                    item.span().map(|span| {
                        [
                            text[..span.start].encode_utf16().count(),
                            text[..span.end].encode_utf16().count(),
                        ]
                    })
                };
            }
        };
    }
    let span = item.span()?;
    Some([
        text[..span.start].encode_utf16().count(),
        text[..span.end].encode_utf16().count(),
    ])
}

#[cfg(test)]
#[path = "comparison_tests.rs"]
mod tests;
