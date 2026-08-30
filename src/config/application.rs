//! One-shot Config Application and Last Application drift observation.

use super::catalog::{inspect_named_config_directory, read_named_config_definition};
use super::files::{
    capture_optional_agent_file, snapshot_text, temporary_file_prefix, write_temporary_file,
};
use super::{
    ApplicationStatus, ConfigDrift, LAST_APPLICATION_SECTION, LastApplication, NamedConfigName,
};
use crate::foundation::safe_fs::FileSnapshot;
use crate::metadata::{self, PreparedMetadataWrite};
use crate::tenant::{self, TenantAgent};
use anyhow::{Context, Result};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub(crate) fn apply_named_config(selected: &TenantAgent, config: &NamedConfigName) -> Result<()> {
    let definition = read_named_config_definition(selected, config)?;
    let current_main = capture_optional_agent_file(selected, selected.agent().main_config_file())?;
    let current_auth = selected
        .agent()
        .native_auth_file()
        .map(|file| capture_optional_agent_file(selected, file))
        .transpose()?;
    let main_text = snapshot_text(&current_main, selected.agent().main_config_file())?;
    let auth_text = current_auth
        .as_ref()
        .map(|snapshot| snapshot_text(snapshot, "auth.json"))
        .transpose()?
        .flatten();
    let desired = definition.apply(main_text.as_deref(), auth_text.as_deref())?;
    let metadata = prepare_last_application(selected, config.as_str())?;

    let mut writes = Vec::new();
    collect_agent_write(
        selected.agent().main_config_file(),
        &current_main,
        desired.main,
        &mut writes,
    );
    if let Some(file) = selected.agent().native_auth_file() {
        // A missing Current auth file is still a writable target: applying a
        // Named Config must materialize its complete native auth object.
        let absent = FileSnapshot {
            present: false,
            content: Vec::new(),
            mode: None,
        };
        let current = current_auth.as_ref().unwrap_or(&absent);
        collect_agent_write(file, current, desired.auth, &mut writes);
    }
    if !writes.is_empty() {
        tenant::ensure_agent_state(selected.agent(), selected.home_dir())?;
        let mut prepared = Vec::with_capacity(writes.len());
        for write in writes {
            let target = selected.state_file(write.file);
            let parent = target
                .parent()
                .context("Current Config path has no parent")?;
            let prefix = temporary_file_prefix(&target, "apply")?;
            let temp = write_temporary_file(parent, &prefix, &write.content, write.mode)?;
            prepared.push((target, temp));
        }
        for (target, temp) in prepared {
            let parent = target
                .parent()
                .context("Current Config path has no parent")?;
            temp.persist(&target, "replace")?;
            crate::foundation::safe_fs::sync_dir(parent)?;
        }
    }
    metadata.commit().context("write Last Application metadata")
}

pub(crate) fn application_status(selected: &TenantAgent) -> ApplicationStatus {
    match application_status_inner(selected) {
        Ok(status) => status,
        Err(error) => ApplicationStatus {
            last_application: None,
            drift: ConfigDrift::ComparisonError,
            detail: Some(format!("{error:#}")),
        },
    }
}

fn application_status_inner(selected: &TenantAgent) -> Result<ApplicationStatus> {
    let Some(last_application) = read_last_application(selected)? else {
        return Ok(ApplicationStatus {
            last_application: None,
            drift: ConfigDrift::Untracked,
            detail: None,
        });
    };
    let applied = NamedConfigName::parse(&last_application.applied)?;
    let layout = match inspect_named_config_directory(selected, &applied) {
        Ok(Some(layout)) if layout.complete(selected) => layout,
        Ok(_) => {
            return Ok(ApplicationStatus {
                last_application: Some(last_application),
                drift: ConfigDrift::SourceMissing,
                detail: None,
            });
        }
        Err(error) => {
            return Ok(ApplicationStatus {
                last_application: Some(last_application),
                drift: ConfigDrift::ComparisonError,
                detail: Some(format!("{error:#}")),
            });
        }
    };
    let _ = layout;
    let comparison = compare_application_source(selected, &applied);
    Ok(match comparison {
        Ok(clean) => ApplicationStatus {
            last_application: Some(last_application),
            drift: if clean {
                ConfigDrift::Clean
            } else {
                ConfigDrift::Dirty
            },
            detail: None,
        },
        Err(error) => ApplicationStatus {
            last_application: Some(last_application),
            drift: ConfigDrift::ComparisonError,
            detail: Some(format!("{error:#}")),
        },
    })
}

fn compare_application_source(selected: &TenantAgent, config: &NamedConfigName) -> Result<bool> {
    let definition = read_named_config_definition(selected, config)?;
    let current_main = capture_optional_agent_file(selected, selected.agent().main_config_file())?;
    let current_auth = selected
        .agent()
        .native_auth_file()
        .map(|file| capture_optional_agent_file(selected, file))
        .transpose()?;
    let main_text = snapshot_text(&current_main, selected.agent().main_config_file())?;
    let auth_text = current_auth
        .as_ref()
        .map(|snapshot| snapshot_text(snapshot, "auth.json"))
        .transpose()?
        .flatten();
    let desired = definition.apply(main_text.as_deref(), auth_text.as_deref())?;
    let main_matches = desired_file_matches(&current_main, desired.main.as_deref());
    let auth_matches = match (selected.agent().native_auth_file(), current_auth.as_ref()) {
        (Some(_), Some(current)) => desired_file_matches(current, desired.auth.as_deref()),
        (None, None) => true,
        _ => false,
    };
    Ok(main_matches && auth_matches)
}

fn desired_file_matches(current: &FileSnapshot, desired: Option<&str>) -> bool {
    match desired {
        Some(desired) => current.present && current.content == desired.as_bytes(),
        None => !current.present,
    }
}

fn prepare_last_application(selected: &TenantAgent, config: &str) -> Result<PreparedMetadataWrite> {
    let mut document = metadata::read(selected)?;
    if let Some(existing) = document.section::<LastApplication>(LAST_APPLICATION_SECTION)? {
        validate_last_application(&existing)?;
    }
    let record = LastApplication {
        applied: config.to_string(),
        applied_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .context("format Last Application time")?,
    };
    document.set_section(LAST_APPLICATION_SECTION, &record)?;
    document.prepare(selected)
}

fn read_last_application(selected: &TenantAgent) -> Result<Option<LastApplication>> {
    let document = metadata::read(selected)?;
    let Some(record): Option<LastApplication> = document.section(LAST_APPLICATION_SECTION)? else {
        return Ok(None);
    };
    validate_last_application(&record)?;
    Ok(Some(record))
}

fn validate_last_application(record: &LastApplication) -> Result<()> {
    tenant::validate_name("config", &record.applied)?;
    OffsetDateTime::parse(&record.applied_at, &Rfc3339).context("parse Last Application time")?;
    Ok(())
}

struct AgentWrite {
    file: &'static str,
    content: Vec<u8>,
    mode: u32,
}

fn collect_agent_write(
    file: &'static str,
    current: &FileSnapshot,
    desired: Option<String>,
    writes: &mut Vec<AgentWrite>,
) {
    let Some(desired) = desired else {
        debug_assert!(!current.present);
        return;
    };
    let content = desired.into_bytes();
    if current.present && current.content == content {
        return;
    }
    writes.push(AgentWrite {
        file,
        content,
        mode: current.mode.unwrap_or(0o600),
    });
}
