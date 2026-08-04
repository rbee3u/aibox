//! Pure Agent Configuration parsing, Agent Profile ownership, and three-way
//! merge.

use crate::agent::AgentKind;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Private metadata file stored beside native Agent Profile files.
pub(crate) const PROFILE_METADATA_FILE: &str = ".metadata.json";

/// One logical JSON Pointer within a normalized configuration tree.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) struct Pointer(Vec<String>);

impl Pointer {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        if value.is_empty() || !value.starts_with('/') {
            bail!("configuration path must be a nonempty JSON Pointer: {value:?}");
        }
        let segments = value[1..]
            .split('/')
            .map(|segment| decode_pointer_segment(segment, value))
            .collect::<Result<Vec<_>>>()?;
        let pointer = Self(segments);
        pointer.validate_domain_path()?;
        Ok(pointer)
    }

    fn from_segments(segments: Vec<String>) -> Self {
        Self(segments)
    }

    fn root() -> Self {
        Self(Vec::new())
    }

    fn domain(domain: &str) -> Self {
        Self(vec![domain.to_string()])
    }

    fn child(&self, segment: &str) -> Self {
        let mut segments = self.0.clone();
        segments.push(segment.to_string());
        Self(segments)
    }

    fn segments(&self) -> &[String] {
        &self.0
    }

    fn validate_domain_path(&self) -> Result<()> {
        match self.0.as_slice() {
            [root, ..] if root == "config" || root == "auth" => Ok(()),
            _ => bail!(
                "configuration path must start with /config or /auth: {}",
                self.display_for_terminal()
            ),
        }
    }

    pub(crate) fn is_auth(&self) -> bool {
        self.0.first().is_some_and(|segment| segment == "auth")
    }

    /// Render a JSON Pointer without allowing owned configuration keys to
    /// inject terminal control sequences into status, diff, or diagnostics.
    pub(crate) fn display_for_terminal(&self) -> String {
        let mut output = String::new();
        for character in self.to_string().chars() {
            if character.is_control() {
                output.extend(character.escape_default());
            } else {
                output.push(character);
            }
        }
        output
    }
}

impl fmt::Display for Pointer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for segment in &self.0 {
            write!(
                formatter,
                "/{}",
                segment.replace('~', "~0").replace('/', "~1")
            )?;
        }
        Ok(())
    }
}

fn decode_pointer_segment(segment: &str, pointer: &str) -> Result<String> {
    let mut output = String::new();
    let mut chars = segment.chars();
    while let Some(character) = chars.next() {
        if character != '~' {
            output.push(character);
            continue;
        }
        match chars.next() {
            Some('0') => output.push('~'),
            Some('1') => output.push('/'),
            _ => bail!("invalid JSON Pointer escape in {pointer:?}"),
        }
    }
    Ok(output)
}

/// An Agent Profile-owned node. Objects carry independently mergeable children;
/// scalar/array/type replacements and empty objects are atomic values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum OverlayNode {
    Object(BTreeMap<String, OverlayNode>),
    Value(Value),
    Tombstone,
}

/// Parsed native Agent Profile definition plus aibox-owned tombstones.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProfileDefinition {
    root: OverlayNode,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProfileMetadata {
    #[serde(default)]
    tombstones: Vec<String>,
}

/// One three-way relationship at a logical path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChangeClass {
    WorkingOnly,
    SourceOnly,
    BothSame,
    Conflict,
}

impl fmt::Display for ChangeClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::WorkingOnly => "working-only",
            Self::SourceOnly => "source-only",
            Self::BothSame => "both-same",
            Self::Conflict => "conflict",
        };
        formatter.write_str(value)
    }
}

/// A classified logical path.
#[derive(Clone, Debug)]
pub(crate) struct Change {
    pub path: Pointer,
    pub class: ChangeClass,
}

/// Explicit selections for conflicting paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConflictChoice {
    Profile,
    Config,
}

/// Result of a three-way Agent Profile reconciliation.
#[derive(Debug)]
pub(crate) struct ReconcileResult {
    pub changes: Vec<Change>,
    pub merged: ProfileDefinition,
}

/// One old/new entry for human-readable diff output.
#[derive(Clone, Debug)]
pub(crate) struct DiffEntry {
    pub path: Pointer,
    pub old: Option<OverlayNode>,
    pub new: Option<OverlayNode>,
}

impl ProfileDefinition {
    pub(crate) fn empty() -> Self {
        Self {
            root: OverlayNode::Object(BTreeMap::new()),
        }
    }

    /// Parse native main/auth files and the private tombstone sidecar.
    pub(crate) fn parse(
        agent: AgentKind,
        main: &str,
        auth: &str,
        metadata: Option<&str>,
    ) -> Result<Self> {
        let config =
            parse_main_config(agent, main).context("parse Agent Profile main configuration")?;
        let auth = parse_json_object(auth, "Agent Profile auth.json")?;
        if agent == AgentKind::Claude && auth.values().any(|value| !value.is_string()) {
            bail!("Claude Agent Profile auth.json must be an object of string values");
        }
        if agent == AgentKind::Claude {
            if let Some(env) = config.get("env").and_then(Value::as_object) {
                if let Some(key) = auth.keys().find(|key| env.contains_key(*key)) {
                    bail!(
                        "Claude Agent Profile credential {key:?} is declared in both settings.json env and auth.json"
                    );
                }
            }
        }

        let mut root = BTreeMap::new();
        if !config.is_empty() {
            root.insert("config".to_string(), object_to_overlay(config));
        }
        if !auth.is_empty() {
            let auth_node = if agent == AgentKind::Codex {
                OverlayNode::Value(Value::Object(auth))
            } else {
                object_to_overlay(auth)
            };
            root.insert("auth".to_string(), auth_node);
        }
        let mut definition = Self {
            root: OverlayNode::Object(root),
        };

        let metadata = match metadata {
            Some(content) if !content.trim().is_empty() => {
                serde_json::from_str::<ProfileMetadata>(content)
                    .context("parse Agent Profile tombstone metadata")?
            }
            _ => ProfileMetadata {
                tombstones: Vec::new(),
            },
        };
        let mut seen = BTreeSet::new();
        for path in metadata.tombstones {
            let path = Pointer::parse(&path)?;
            if agent == AgentKind::Codex && path.is_auth() && path.segments().len() != 1 {
                bail!("Codex auth ownership is whole-file at /auth");
            }
            if !seen.insert(path.clone()) {
                bail!(
                    "duplicate Agent Profile tombstone: {}",
                    path.display_for_terminal()
                );
            }
            if definition.node_at(&path).is_some()
                || definition.has_owned_ancestor(&path)
                || definition.has_owned_descendant(&path)
            {
                bail!(
                    "Agent Profile tombstone overlaps a declared value: {}",
                    path.display_for_terminal()
                );
            }
            definition.set_node(&path, OverlayNode::Tombstone)?;
        }
        Ok(definition)
    }

    /// Render the normalized definition back into native Agent Profile files.
    pub(crate) fn render(&self, agent: AgentKind) -> Result<(String, String, String)> {
        let mut tombstones = Vec::new();
        let config = self.render_domain_object("config", &mut tombstones)?;
        let auth = self.render_domain_object("auth", &mut tombstones)?;
        if agent == AgentKind::Claude && auth.values().any(|value| !value.is_string()) {
            bail!("Claude Agent Profile auth values must remain strings");
        }
        let main = render_main_config(agent, &Value::Object(config))?;
        let auth = format!("{}\n", serde_json::to_string_pretty(&Value::Object(auth))?);
        tombstones.sort();
        let metadata = format!(
            "{}\n",
            serde_json::to_string_pretty(&ProfileMetadata { tombstones })?
        );
        Ok((main, auth, metadata))
    }

    fn render_domain_object(
        &self,
        domain: &str,
        tombstones: &mut Vec<String>,
    ) -> Result<Map<String, Value>> {
        let path = Pointer::domain(domain);
        let Some(node) = self.node_at(&path) else {
            return Ok(Map::new());
        };
        match render_overlay_node(node, &path, tombstones)? {
            Some(Value::Object(object)) => Ok(object),
            Some(_) => bail!("Agent Profile /{domain} must render as an object"),
            None => Ok(Map::new()),
        }
    }

    /// Paths under `/auth` used to split Claude `settings.env` into the logical
    /// credential tree.
    pub(crate) fn auth_keys(&self) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        let auth = Pointer::domain("auth");
        if let Some(OverlayNode::Object(values)) = self.node_at(&auth) {
            keys.extend(values.keys().cloned());
        }
        keys
    }

    pub(crate) fn owns_domain(&self, domain: &str) -> bool {
        let path = Pointer::domain(domain);
        self.node_at(&path).is_some()
    }

    pub(crate) fn deletes_domain(&self, domain: &str) -> bool {
        let path = Pointer::domain(domain);
        matches!(self.node_at(&path), Some(OverlayNode::Tombstone))
    }

    /// Whether this definition owns a path, one of its ancestors, or one of
    /// its descendants.
    pub(crate) fn overlaps_path(&self, path: &Pointer) -> bool {
        self.node_at(path).is_some()
            || self.has_owned_ancestor(path)
            || self.has_owned_descendant(path)
    }

    fn node_at(&self, path: &Pointer) -> Option<&OverlayNode> {
        let mut node = &self.root;
        for segment in path.segments() {
            let OverlayNode::Object(children) = node else {
                return None;
            };
            node = children.get(segment)?;
        }
        Some(node)
    }

    fn has_owned_ancestor(&self, path: &Pointer) -> bool {
        let mut node = &self.root;
        for segment in path.segments() {
            match node {
                OverlayNode::Object(children) => {
                    let Some(next) = children.get(segment) else {
                        return false;
                    };
                    node = next;
                }
                OverlayNode::Value(_) | OverlayNode::Tombstone => return true,
            }
        }
        false
    }

    fn has_owned_descendant(&self, path: &Pointer) -> bool {
        matches!(self.node_at(path), Some(OverlayNode::Object(children)) if !children.is_empty())
    }

    fn set_node(&mut self, path: &Pointer, value: OverlayNode) -> Result<()> {
        set_overlay_node(&mut self.root, path.segments(), value)
    }
}

fn object_to_overlay(object: Map<String, Value>) -> OverlayNode {
    OverlayNode::Object(
        object
            .into_iter()
            .map(|(key, value)| {
                let node = match value {
                    Value::Object(object) if !object.is_empty() => object_to_overlay(object),
                    value => OverlayNode::Value(value),
                };
                (key, node)
            })
            .collect(),
    )
}

fn render_overlay_node(
    node: &OverlayNode,
    path: &Pointer,
    tombstones: &mut Vec<String>,
) -> Result<Option<Value>> {
    match node {
        OverlayNode::Tombstone => {
            tombstones.push(path.to_string());
            Ok(None)
        }
        OverlayNode::Value(value) => Ok(Some(value.clone())),
        OverlayNode::Object(children) => {
            let mut object = Map::new();
            for (key, child) in children {
                if let Some(value) = render_overlay_node(child, &path.child(key), tombstones)? {
                    object.insert(key.clone(), value);
                }
            }
            Ok(Some(Value::Object(object)))
        }
    }
}

fn set_overlay_node(root: &mut OverlayNode, segments: &[String], value: OverlayNode) -> Result<()> {
    let Some((first, rest)) = segments.split_first() else {
        *root = value;
        return Ok(());
    };
    let OverlayNode::Object(children) = root else {
        bail!("configuration ownership paths overlap at {first:?}");
    };
    if rest.is_empty() {
        children.insert(first.clone(), value);
        return Ok(());
    }
    let child = children
        .entry(first.clone())
        .or_insert_with(|| OverlayNode::Object(BTreeMap::new()));
    set_overlay_node(child, rest, value)
}

/// Parse a native main configuration into a JSON object.
pub(crate) fn parse_main_config(agent: AgentKind, content: &str) -> Result<Map<String, Value>> {
    if content.trim().is_empty() {
        return Ok(Map::new());
    }
    let value = match agent {
        AgentKind::Codex => toml_edit::de::from_str::<Value>(content)?,
        AgentKind::Claude => serde_json::from_str::<Value>(content)?,
    };
    value
        .as_object()
        .cloned()
        .with_context(|| format!("{} main configuration must be an object", agent.tag()))
}

/// Render a normalized main configuration in the Coding Agent's native format.
pub(crate) fn render_main_config(agent: AgentKind, value: &Value) -> Result<String> {
    if !value.is_object() {
        bail!("{} main configuration must be an object", agent.tag());
    }
    match agent {
        AgentKind::Codex => Ok(toml_edit::ser::to_string_pretty(value)?),
        AgentKind::Claude => Ok(format!("{}\n", serde_json::to_string_pretty(value)?)),
    }
}

/// Parse a JSON object, treating empty content as `{}`.
pub(crate) fn parse_json_object(content: &str, label: &str) -> Result<Map<String, Value>> {
    let value = if content.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str::<Value>(content).with_context(|| format!("parse {label}"))?
    };
    value
        .as_object()
        .cloned()
        .with_context(|| format!("{label} must be a JSON object"))
}

/// Build the normalized effective tree from native Agent Configuration values.
pub(crate) fn effective_tree(
    agent: AgentKind,
    main: &str,
    auth: Option<&str>,
    claude_auth_keys: &BTreeSet<String>,
) -> Result<Value> {
    let mut config = parse_main_config(agent, main).context("parse Agent Configuration")?;
    let mut root = Map::new();
    match agent {
        AgentKind::Codex => {
            root.insert("config".to_string(), Value::Object(config));
            let auth = parse_json_object(auth.unwrap_or(""), "Agent Configuration auth.json")?;
            root.insert("auth".to_string(), Value::Object(auth));
        }
        AgentKind::Claude => {
            let mut logical_auth = Map::new();
            if let Some(Value::Object(env)) = config.get_mut("env") {
                for key in claude_auth_keys {
                    if let Some(value) = env.remove(key) {
                        logical_auth.insert(key.clone(), value);
                    }
                }
                if env.is_empty() {
                    config.remove("env");
                }
            }
            root.insert("config".to_string(), Value::Object(config));
            root.insert("auth".to_string(), Value::Object(logical_auth));
        }
    }
    Ok(Value::Object(root))
}

/// Render a normalized effective tree to native Agent Configuration content.
pub(crate) fn render_effective(agent: AgentKind, tree: &Value) -> Result<(String, Option<String>)> {
    let object = tree
        .as_object()
        .context("normalized Agent Configuration must be an object")?;
    let mut config = object
        .get("config")
        .and_then(Value::as_object)
        .cloned()
        .context("normalized Agent Configuration needs /config object")?;
    let auth = object
        .get("auth")
        .and_then(Value::as_object)
        .cloned()
        .context("normalized Agent Configuration needs /auth object")?;
    match agent {
        AgentKind::Codex => Ok((
            render_main_config(agent, &Value::Object(config))?,
            Some(format!(
                "{}\n",
                serde_json::to_string_pretty(&Value::Object(auth))?
            )),
        )),
        AgentKind::Claude => {
            if !auth.is_empty() {
                let env = config
                    .entry("env".to_string())
                    .or_insert_with(|| Value::Object(Map::new()));
                let env = env
                    .as_object_mut()
                    .context("Claude settings.env must be an object")?;
                for (key, value) in auth {
                    env.insert(key, value);
                }
            }
            Ok((render_main_config(agent, &Value::Object(config))?, None))
        }
    }
}

/// Apply Agent Profile ownership to a normalized base Agent Configuration.
pub(crate) fn materialize(base: &Value, profile: &ProfileDefinition) -> Result<Value> {
    let mut output = base.clone();
    apply_overlay(&mut output, &profile.root, &Pointer::root())?;
    Ok(output)
}

fn apply_overlay(target: &mut Value, overlay: &OverlayNode, path: &Pointer) -> Result<()> {
    let OverlayNode::Object(children) = overlay else {
        bail!("Agent Profile root must be an object");
    };
    for (key, child) in children {
        let child_path = path.child(key);
        match child {
            // `/config` and `/auth` are structural roots required by the
            // normalized effective tree. A domain tombstone owns absence of
            // the corresponding native file, while an empty object keeps the
            // in-memory tree renderable and comparable.
            OverlayNode::Tombstone if child_path.segments().len() == 1 => {
                set_effective(target, &child_path, Value::Object(Map::new()))?;
            }
            OverlayNode::Tombstone => remove_effective(target, &child_path),
            OverlayNode::Value(value) => set_effective(target, &child_path, value.clone())?,
            OverlayNode::Object(_) => {
                match effective_at(target, &child_path) {
                    Some(Value::Object(_)) => {}
                    None => {
                        set_effective(target, &child_path, Value::Object(Map::new()))?;
                    }
                    Some(_) => {
                        bail!(
                            "Agent Profile path {} crosses an unowned non-object Agent Configuration value",
                            child_path.display_for_terminal()
                        );
                    }
                }
                apply_overlay(target, child, &child_path)?;
            }
        }
    }
    Ok(())
}

fn effective_at<'a>(tree: &'a Value, path: &Pointer) -> Option<&'a Value> {
    let mut value = tree;
    for segment in path.segments() {
        value = value.as_object()?.get(segment)?;
    }
    Some(value)
}

fn set_effective(tree: &mut Value, path: &Pointer, value: Value) -> Result<()> {
    let Some((last, parents)) = path.segments().split_last() else {
        *tree = value;
        return Ok(());
    };
    let mut current = tree;
    for segment in parents {
        let object = current
            .as_object_mut()
            .context("configuration path crosses a non-object value")?;
        current = object
            .entry(segment.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    current
        .as_object_mut()
        .context("configuration path parent is not an object")?
        .insert(last.clone(), value);
    Ok(())
}

fn remove_effective(tree: &mut Value, path: &Pointer) {
    let Some((last, parents)) = path.segments().split_last() else {
        return;
    };
    let mut current = tree;
    for segment in parents {
        let Some(next) = current
            .as_object_mut()
            .and_then(|object| object.get_mut(segment))
        else {
            return;
        };
        current = next;
    }
    if let Some(object) = current.as_object_mut() {
        object.remove(last);
    }
}

/// Copy selected logical values, including absence, between effective trees.
pub(crate) fn copy_effective_paths(
    source: &Value,
    target: &mut Value,
    paths: &[Pointer],
) -> Result<()> {
    for path in paths {
        if let Some(value) = effective_at(source, path) {
            set_effective(target, path, value.clone())?;
        } else {
            remove_effective(target, path);
            for length in (1..path.segments().len()).rev() {
                let ancestor = Pointer::from_segments(path.segments()[..length].to_vec());
                let target_is_empty = effective_at(target, &ancestor)
                    .and_then(Value::as_object)
                    .is_some_and(Map::is_empty);
                if target_is_empty && effective_at(source, &ancestor).is_none() {
                    remove_effective(target, &ancestor);
                }
            }
        }
    }
    Ok(())
}

/// Infer the working Agent Profile side by applying effective changes to the
/// last applied ownership tree.
pub(crate) fn working_definition(
    agent: AgentKind,
    applied: &ProfileDefinition,
    expected: &Value,
    working: &Value,
) -> Result<ProfileDefinition> {
    let mut root = applied.root.clone();
    apply_effective_diff(agent, &mut root, expected, working, &Pointer::root())?;
    Ok(ProfileDefinition { root })
}

fn apply_effective_diff(
    agent: AgentKind,
    overlay: &mut OverlayNode,
    old: &Value,
    new: &Value,
    path: &Pointer,
) -> Result<()> {
    if old == new {
        return Ok(());
    }
    let existing_is_atomic = overlay_node_at(overlay, path.segments())
        .is_some_and(|node| matches!(node, OverlayNode::Value(_) | OverlayNode::Tombstone));
    let codex_auth_is_atomic = agent == AgentKind::Codex && path.segments() == ["auth"];
    if !path.segments().is_empty() && (existing_is_atomic || codex_auth_is_atomic) {
        let replacement = if codex_auth_is_atomic {
            if new.as_object().is_some_and(Map::is_empty) {
                // An empty Codex auth object is semantically unauthenticated.
                // The Agent Profile format uses `/auth` as the whole-file
                // deletion marker; plain `{}` intentionally means that an
                // Agent Profile does not own the native auth file.
                OverlayNode::Tombstone
            } else {
                OverlayNode::Value(new.clone())
            }
        } else {
            // Native Agent Profile files canonicalize non-empty objects into
            // independently owned children. Mirror that representation now so
            // a reconcile does not appear divergent immediately after the
            // rendered source is parsed again.
            overlay_for_addition(new)
        };
        set_overlay_node(overlay, path.segments(), replacement)?;
        return Ok(());
    }
    if let (Some(old), Some(new)) = (old.as_object(), new.as_object()) {
        let keys: BTreeSet<_> = old.keys().chain(new.keys()).cloned().collect();
        for key in keys {
            let child_path = path.child(&key);
            match (old.get(&key), new.get(&key)) {
                (Some(old), Some(new)) => {
                    apply_effective_diff(agent, overlay, old, new, &child_path)?;
                }
                (None, Some(new)) => {
                    set_overlay_node(overlay, child_path.segments(), overlay_for_addition(new))?;
                }
                (Some(_), None) => {
                    set_overlay_node(overlay, child_path.segments(), OverlayNode::Tombstone)?;
                }
                (None, None) => unreachable!(),
            }
        }
    } else {
        set_overlay_node(overlay, path.segments(), overlay_for_addition(new))?;
    }
    Ok(())
}

fn overlay_node_at<'a>(root: &'a OverlayNode, segments: &[String]) -> Option<&'a OverlayNode> {
    let mut node = root;
    for segment in segments {
        let OverlayNode::Object(children) = node else {
            return None;
        };
        node = children.get(segment)?;
    }
    Some(node)
}

fn overlay_for_addition(value: &Value) -> OverlayNode {
    match value {
        Value::Object(object) if !object.is_empty() => object_to_overlay(object.clone()),
        value => OverlayNode::Value(value.clone()),
    }
}

/// Classify and merge applied, working, and source Agent Profile definitions.
pub(crate) fn reconcile(
    applied: &ProfileDefinition,
    working: &ProfileDefinition,
    source: &ProfileDefinition,
    resolutions: &BTreeMap<Pointer, ConflictChoice>,
) -> Result<ReconcileResult> {
    let mut changes = Vec::new();
    let root_path = Pointer::root();
    let merged = merge_node(
        Some(&applied.root),
        Some(&working.root),
        Some(&source.root),
        &root_path,
        resolutions,
        &mut changes,
    )?
    .unwrap_or_else(|| OverlayNode::Object(BTreeMap::new()));
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    let conflicts: BTreeSet<_> = changes
        .iter()
        .filter(|change| change.class == ChangeClass::Conflict)
        .map(|change| change.path.clone())
        .collect();
    for path in resolutions.keys() {
        if !conflicts.contains(path) {
            bail!(
                "resolution path is not a current conflict: {}",
                path.display_for_terminal()
            );
        }
    }
    Ok(ReconcileResult {
        changes,
        merged: ProfileDefinition { root: merged },
    })
}

fn merge_node(
    applied: Option<&OverlayNode>,
    working: Option<&OverlayNode>,
    source: Option<&OverlayNode>,
    path: &Pointer,
    resolutions: &BTreeMap<Pointer, ConflictChoice>,
    changes: &mut Vec<Change>,
) -> Result<Option<OverlayNode>> {
    if working == applied && source == applied {
        return Ok(applied.cloned());
    }
    // Objects are structural containers rather than atomic owned values.
    // Always recurse through them so one-sided changes are classified at their
    // actual JSON Pointers. Scalars, arrays, and structural replacements still
    // take the one-sided or conflict paths below as atomic values.
    if can_merge_as_objects(&[applied, working, source]) {
        let applied_children = object_children(applied);
        let working_children = object_children(working);
        let source_children = object_children(source);
        let keys: BTreeSet<_> = applied_children
            .keys()
            .chain(working_children.keys())
            .chain(source_children.keys())
            .cloned()
            .collect();
        let mut merged = BTreeMap::new();
        for key in keys {
            if let Some(node) = merge_node(
                applied_children.get(&key).copied(),
                working_children.get(&key).copied(),
                source_children.get(&key).copied(),
                &path.child(&key),
                resolutions,
                changes,
            )? {
                merged.insert(key, node);
            }
        }
        // Non-empty native objects are represented structurally so their
        // children can merge independently. An empty native object is instead
        // an atomic `Value`, so an empty structural container below the root
        // can only be merge residue after its final owned child was removed.
        // Prune it to keep the result in the same canonical form that parsing
        // the rendered Agent Profile will produce.
        if merged.is_empty() && !path.segments().is_empty() {
            return Ok(None);
        }
        return Ok(Some(OverlayNode::Object(merged)));
    }

    if !path.segments().is_empty() {
        if working == applied {
            record_change(changes, path, ChangeClass::SourceOnly);
            return Ok(source.cloned());
        }
        if source == applied {
            record_change(changes, path, ChangeClass::WorkingOnly);
            return Ok(working.cloned());
        }
        if working == source {
            record_change(changes, path, ChangeClass::BothSame);
            return Ok(working.cloned());
        }
    }

    record_change(changes, path, ChangeClass::Conflict);
    match resolutions.get(path) {
        Some(ConflictChoice::Profile) => Ok(source.cloned()),
        Some(ConflictChoice::Config) => Ok(working.cloned()),
        None => Ok(applied.cloned()),
    }
}

fn record_change(changes: &mut Vec<Change>, path: &Pointer, class: ChangeClass) {
    if path.segments().is_empty() {
        return;
    }
    changes.push(Change {
        path: path.clone(),
        class,
    });
}

fn can_merge_as_objects(nodes: &[Option<&OverlayNode>]) -> bool {
    nodes
        .iter()
        .copied()
        .flatten()
        .all(|node| matches!(node, OverlayNode::Object(_)))
}

fn object_children(node: Option<&OverlayNode>) -> BTreeMap<String, &OverlayNode> {
    match node {
        Some(OverlayNode::Object(children)) => children
            .iter()
            .map(|(key, value)| (key.clone(), value))
            .collect(),
        None => BTreeMap::new(),
        Some(_) => unreachable!("caller checked object nodes"),
    }
}

/// Compare one Agent Profile ownership tree to another.
pub(crate) fn diff(old: &ProfileDefinition, new: &ProfileDefinition) -> Vec<DiffEntry> {
    let mut entries = Vec::new();
    diff_node(
        Some(&old.root),
        Some(&new.root),
        &Pointer::root(),
        &mut entries,
    );
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries
}

fn diff_node(
    old: Option<&OverlayNode>,
    new: Option<&OverlayNode>,
    path: &Pointer,
    entries: &mut Vec<DiffEntry>,
) {
    if old == new {
        return;
    }
    if can_merge_as_objects(&[old, new]) {
        let old_children = object_children(old);
        let new_children = object_children(new);
        let keys: BTreeSet<_> = old_children
            .keys()
            .chain(new_children.keys())
            .cloned()
            .collect();
        for key in keys {
            diff_node(
                old_children.get(&key).copied(),
                new_children.get(&key).copied(),
                &path.child(&key),
                entries,
            );
        }
    } else if !path.segments().is_empty() {
        entries.push(DiffEntry {
            path: path.clone(),
            old: old.cloned(),
            new: new.cloned(),
        });
    }
}

/// Render an owned value for diff output. Callers redact auth paths.
pub(crate) fn display_node(node: Option<&OverlayNode>) -> String {
    match node {
        None => "<unowned>".to_string(),
        Some(OverlayNode::Tombstone) => "<deleted>".to_string(),
        Some(OverlayNode::Value(value)) => value.to_string(),
        Some(OverlayNode::Object(children)) => {
            let mut object = Map::new();
            for (key, value) in children {
                object.insert(key.clone(), overlay_to_display_value(value));
            }
            Value::Object(object).to_string()
        }
    }
}

fn overlay_to_display_value(node: &OverlayNode) -> Value {
    match node {
        OverlayNode::Tombstone => Value::String("<deleted>".to_string()),
        OverlayNode::Value(value) => value.clone(),
        OverlayNode::Object(children) => Value::Object(
            children
                .iter()
                .map(|(key, value)| (key.clone(), overlay_to_display_value(value)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::only;
    use serde_json::json;

    fn profile(agent: AgentKind, main: &str, auth: &str, tombstones: &[&str]) -> ProfileDefinition {
        let metadata = serde_json::json!({"tombstones": tombstones}).to_string();
        ProfileDefinition::parse(agent, main, auth, Some(&metadata)).unwrap()
    }

    #[test]
    fn pointers_round_trip_escaped_segments() {
        let path = Pointer::parse("/config/a~1b/~0value").unwrap();
        assert_eq!(path.segments(), &["config", "a/b", "~value"]);
        assert_eq!(path.to_string(), "/config/a~1b/~0value");
    }

    #[test]
    fn pointer_validation_rejects_malformed_or_out_of_domain_paths() {
        for path in [
            "",
            "config/model",
            "/",
            "/other/model",
            "/config/~",
            "/config/~2",
        ] {
            assert!(Pointer::parse(path).is_err(), "{path:?} should be rejected");
        }
    }

    #[test]
    fn terminal_pointer_display_escapes_control_characters() {
        let path =
            Pointer::from_segments(vec!["config".to_string(), "line\n\u{1b}[31m".to_string()]);

        assert_eq!(path.to_string(), "/config/line\n\u{1b}[31m");
        assert_eq!(path.display_for_terminal(), "/config/line\\n\\u{1b}[31m");
    }

    #[test]
    fn claude_auth_is_separate_and_materializes_into_env() {
        let definition = profile(
            AgentKind::Claude,
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://api.example"}}"#,
            r#"{"ANTHROPIC_AUTH_TOKEN":"secret"}"#,
            &[],
        );
        let base = effective_tree(AgentKind::Claude, "{}", None, &definition.auth_keys()).unwrap();
        let effective = materialize(&base, &definition).unwrap();
        let (settings, auth) = render_effective(AgentKind::Claude, &effective).unwrap();
        assert!(auth.is_none());
        let settings: Value = serde_json::from_str(&settings).unwrap();
        assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "secret");
        assert_eq!(settings["env"]["ANTHROPIC_BASE_URL"], "https://api.example");
    }

    #[test]
    fn profile_tombstones_reject_ambiguous_or_partial_ownership() {
        let cases = [
            (
                AgentKind::Claude,
                r#"{"model":"profile"}"#,
                "{}",
                r#"{"tombstones":["/config/model"]}"#,
                "overlaps a declared value",
            ),
            (
                AgentKind::Claude,
                "{}",
                "{}",
                r#"{"tombstones":["/config/model","/config/model"]}"#,
                "duplicate Agent Profile tombstone",
            ),
            (
                AgentKind::Codex,
                "",
                "{}",
                r#"{"tombstones":["/auth/token"]}"#,
                "whole-file at /auth",
            ),
        ];

        for (agent, main, auth, metadata, expected) in cases {
            let error = ProfileDefinition::parse(agent, main, auth, Some(metadata))
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn claude_credentials_must_be_strings_and_disjoint_from_settings_env() {
        let non_string = ProfileDefinition::parse(
            AgentKind::Claude,
            "{}",
            r#"{"ANTHROPIC_AUTH_TOKEN":42}"#,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(
            non_string.contains("object of string values"),
            "{non_string}"
        );

        let duplicate = ProfileDefinition::parse(
            AgentKind::Claude,
            r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"inline"}}"#,
            r#"{"ANTHROPIC_AUTH_TOKEN":"secret"}"#,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(duplicate.contains("declared in both"), "{duplicate}");
    }

    #[test]
    fn claude_effective_round_trip_extracts_only_profile_owned_credentials() {
        let keys = BTreeSet::from(["ANTHROPIC_AUTH_TOKEN".to_string()]);
        let effective = effective_tree(
            AgentKind::Claude,
            r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"secret","KEEP":"value"},"theme":"dark"}"#,
            None,
            &keys,
        )
        .unwrap();

        assert_eq!(effective["auth"], json!({"ANTHROPIC_AUTH_TOKEN": "secret"}));
        assert_eq!(effective["config"]["env"], json!({"KEEP": "value"}));
        let (settings, auth) = render_effective(AgentKind::Claude, &effective).unwrap();
        assert!(auth.is_none());
        assert_eq!(
            serde_json::from_str::<Value>(&settings).unwrap(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "secret",
                    "KEEP": "value"
                },
                "theme": "dark"
            })
        );
    }

    #[test]
    fn copying_effective_paths_preserves_siblings_and_prunes_absent_ancestors() {
        let status_line = Pointer::parse("/config/tui/status_line").unwrap();
        let source = json!({
            "config": {"tui": {"status_line": ["model"], "use_colors": true}},
            "auth": {}
        });
        let mut target = json!({
            "config": {"tui": {"status_line": ["old"], "use_colors": false}, "keep": 1},
            "auth": {}
        });

        copy_effective_paths(&source, &mut target, std::slice::from_ref(&status_line)).unwrap();
        assert_eq!(target["config"]["tui"]["status_line"], json!(["model"]));
        assert_eq!(target["config"]["tui"]["use_colors"], false);
        assert_eq!(target["config"]["keep"], 1);

        let source = json!({"config": {"keep": true}, "auth": {}});
        let mut target = json!({
            "config": {"tui": {"status_line": ["old"]}, "keep": false},
            "auth": {}
        });
        copy_effective_paths(&source, &mut target, &[status_line]).unwrap();
        assert!(target["config"].get("tui").is_none());
        assert_eq!(target["config"]["keep"], false);
    }

    #[test]
    fn materialization_rejects_overwriting_an_unowned_scalar_ancestor() {
        let definition = profile(
            AgentKind::Codex,
            "[tui]\nstatus_line = [\"model\"]\n",
            "{}",
            &[],
        );
        let base = effective_tree(
            AgentKind::Codex,
            "tui = \"keep\"\n",
            Some(""),
            &BTreeSet::new(),
        )
        .unwrap();

        let error = materialize(&base, &definition).unwrap_err().to_string();
        assert!(error.contains("/config/tui"), "{error}");
        assert!(error.contains("unowned non-object"), "{error}");
    }

    #[test]
    fn working_deletion_becomes_a_tombstone() {
        let applied = profile(AgentKind::Claude, r#"{"model":"old"}"#, "{}", &[]);
        let keys = BTreeSet::new();
        let base = effective_tree(
            AgentKind::Claude,
            r#"{"model":"base","keep":true}"#,
            None,
            &keys,
        )
        .unwrap();
        let expected = materialize(&base, &applied).unwrap();
        let working = effective_tree(AgentKind::Claude, r#"{"keep":true}"#, None, &keys).unwrap();
        let working = working_definition(AgentKind::Claude, &applied, &expected, &working).unwrap();
        let (_, _, metadata) = working.render(AgentKind::Claude).unwrap();
        assert!(metadata.contains("/config/model"));
    }

    #[test]
    fn codex_working_auth_changes_keep_whole_file_ownership() {
        let applied = profile(AgentKind::Codex, "", r#"{"token":"applied"}"#, &[]);
        let base = effective_tree(AgentKind::Codex, "", Some(""), &BTreeSet::new()).unwrap();
        let expected = materialize(&base, &applied).unwrap();
        let working_tree = effective_tree(
            AgentKind::Codex,
            "",
            Some(r#"{"token":"working","account":"new"}"#),
            &BTreeSet::new(),
        )
        .unwrap();

        let working =
            working_definition(AgentKind::Codex, &applied, &expected, &working_tree).unwrap();
        let entries = diff(&applied, &working);
        assert_eq!(only(&entries).path.to_string(), "/auth");
        let (_, auth, metadata) = working.render(AgentKind::Codex).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&auth).unwrap(),
            json!({"account": "new", "token": "working"})
        );
        assert!(!metadata.contains("/auth"));
    }

    #[test]
    fn codex_empty_working_auth_becomes_a_whole_file_tombstone() {
        let applied = profile(AgentKind::Codex, "", r#"{"token":"applied"}"#, &[]);
        let base = effective_tree(AgentKind::Codex, "", Some(""), &BTreeSet::new()).unwrap();
        let expected = materialize(&base, &applied).unwrap();
        let working_tree =
            effective_tree(AgentKind::Codex, "", Some(""), &BTreeSet::new()).unwrap();

        let working =
            working_definition(AgentKind::Codex, &applied, &expected, &working_tree).unwrap();
        let entries = diff(&applied, &working);
        assert_eq!(only(&entries).path.to_string(), "/auth");
        let (_, auth, metadata) = working.render(AgentKind::Codex).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&auth).unwrap(), json!({}));
        assert!(metadata.contains("/auth"));

        let effective = materialize(&base, &working).unwrap();
        assert_eq!(effective["auth"], json!({}));
    }

    #[test]
    fn growing_an_owned_empty_object_uses_the_canonical_structural_form() {
        let applied = profile(AgentKind::Claude, r#"{"service":{}}"#, "{}", &[]);
        let base = effective_tree(AgentKind::Claude, "{}", None, &BTreeSet::new()).unwrap();
        let expected = materialize(&base, &applied).unwrap();
        let working_tree = effective_tree(
            AgentKind::Claude,
            r#"{"service":{"url":"https://example.com"}}"#,
            None,
            &BTreeSet::new(),
        )
        .unwrap();

        let working =
            working_definition(AgentKind::Claude, &applied, &expected, &working_tree).unwrap();
        let (main, auth, metadata) = working.render(AgentKind::Claude).unwrap();
        let reparsed =
            ProfileDefinition::parse(AgentKind::Claude, &main, &auth, Some(&metadata)).unwrap();
        assert_eq!(working, reparsed);
    }

    #[test]
    fn adopting_an_object_replacement_uses_the_canonical_structural_form() {
        let applied = ProfileDefinition::empty();
        let base = effective_tree(
            AgentKind::Claude,
            r#"{"service":"base"}"#,
            None,
            &BTreeSet::new(),
        )
        .unwrap();
        let working_tree = effective_tree(
            AgentKind::Claude,
            r#"{"service":{"url":"https://example.com"}}"#,
            None,
            &BTreeSet::new(),
        )
        .unwrap();

        let working =
            working_definition(AgentKind::Claude, &applied, &base, &working_tree).unwrap();
        let (main, auth, metadata) = working.render(AgentKind::Claude).unwrap();
        let reparsed =
            ProfileDefinition::parse(AgentKind::Claude, &main, &auth, Some(&metadata)).unwrap();
        assert_eq!(working, reparsed);
    }

    #[test]
    fn three_way_merge_auto_merges_non_overlapping_changes() {
        let applied = profile(AgentKind::Claude, r#"{"a":1,"b":1}"#, "{}", &[]);
        let working = profile(AgentKind::Claude, r#"{"a":2,"b":1}"#, "{}", &[]);
        let source = profile(AgentKind::Claude, r#"{"a":1,"b":2}"#, "{}", &[]);
        let result = reconcile(&applied, &working, &source, &BTreeMap::new()).unwrap();
        assert_eq!(result.changes.len(), 2);
        assert_eq!(result.changes[0].class, ChangeClass::WorkingOnly);
        assert_eq!(result.changes[1].class, ChangeClass::SourceOnly);
        let (main, _, _) = result.merged.render(AgentKind::Claude).unwrap();
        let main: Value = serde_json::from_str(&main).unwrap();
        assert_eq!(main, json!({"a": 2, "b": 2}));
    }

    #[test]
    fn identical_changes_on_both_sides_are_classified_and_applied_once() {
        let applied = profile(AgentKind::Claude, r#"{"model":"old"}"#, "{}", &[]);
        let changed = profile(AgentKind::Claude, r#"{"model":"new"}"#, "{}", &[]);

        let result = reconcile(&applied, &changed, &changed, &BTreeMap::new()).unwrap();

        let change = only(&result.changes);
        assert_eq!(change.path.to_string(), "/config/model");
        assert_eq!(change.class, ChangeClass::BothSame);
        let (main, _, _) = result.merged.render(AgentKind::Claude).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&main).unwrap(),
            json!({"model": "new"})
        );
    }

    #[test]
    fn removing_the_last_source_value_drops_structural_ownership() {
        let applied = profile(AgentKind::Claude, r#"{"model":"profile"}"#, "{}", &[]);
        let source = ProfileDefinition::empty();

        let result = reconcile(&applied, &applied, &source, &BTreeMap::new()).unwrap();

        let change = only(&result.changes);
        assert_eq!(change.class, ChangeClass::SourceOnly);
        assert_eq!(change.path.to_string(), "/config/model");
        assert!(!result.merged.owns_domain("config"));
        let (main, auth, metadata) = result.merged.render(AgentKind::Claude).unwrap();
        let reparsed =
            ProfileDefinition::parse(AgentKind::Claude, &main, &auth, Some(&metadata)).unwrap();
        assert_eq!(result.merged, reparsed);
    }

    #[test]
    fn one_sided_top_level_addition_is_classified_at_its_real_path() {
        let applied = profile(AgentKind::Claude, r#"{"model":"a"}"#, "{}", &[]);
        let working = profile(
            AgentKind::Claude,
            r#"{"model":"a","working":true}"#,
            "{}",
            &[],
        );
        let result = reconcile(&applied, &working, &applied, &BTreeMap::new()).unwrap();

        let change = only(&result.changes);
        assert_eq!(change.class, ChangeClass::WorkingOnly);
        assert_eq!(change.path.to_string(), "/config/working");
    }

    #[test]
    fn divergent_scalar_change_is_an_explicit_conflict() {
        let applied = profile(AgentKind::Claude, r#"{"model":"a"}"#, "{}", &[]);
        let working = profile(AgentKind::Claude, r#"{"model":"working"}"#, "{}", &[]);
        let source = profile(AgentKind::Claude, r#"{"model":"source"}"#, "{}", &[]);
        let unresolved = reconcile(&applied, &working, &source, &BTreeMap::new()).unwrap();
        let change = only(&unresolved.changes);
        assert_eq!(change.class, ChangeClass::Conflict);
        assert_eq!(change.path.to_string(), "/config/model");

        let mut choices = BTreeMap::new();
        choices.insert(
            Pointer::parse("/config/model").unwrap(),
            ConflictChoice::Config,
        );
        let resolved = reconcile(&applied, &working, &source, &choices).unwrap();
        let (main, _, _) = resolved.merged.render(AgentKind::Claude).unwrap();
        assert!(main.contains("working"));
    }

    #[test]
    fn deletion_and_modification_conflict_preserves_the_selected_semantics() {
        let applied = profile(AgentKind::Claude, r#"{"model":"applied"}"#, "{}", &[]);
        let working = profile(AgentKind::Claude, "{}", "{}", &["/config/model"]);
        let source = profile(AgentKind::Claude, r#"{"model":"source"}"#, "{}", &[]);

        let unresolved = reconcile(&applied, &working, &source, &BTreeMap::new()).unwrap();
        let change = only(&unresolved.changes);
        assert_eq!(change.path.to_string(), "/config/model");
        assert_eq!(change.class, ChangeClass::Conflict);

        let path = Pointer::parse("/config/model").unwrap();
        let base = effective_tree(
            AgentKind::Claude,
            r#"{"model":"base","keep":true}"#,
            None,
            &BTreeSet::new(),
        )
        .unwrap();
        for (choice, expected_model) in [
            (ConflictChoice::Config, None),
            (ConflictChoice::Profile, Some("source")),
        ] {
            let resolutions = BTreeMap::from([(path.clone(), choice)]);
            let resolved = reconcile(&applied, &working, &source, &resolutions).unwrap();
            let effective = materialize(&base, &resolved.merged).unwrap();
            assert_eq!(
                effective["config"].get("model").and_then(Value::as_str),
                expected_model
            );
            assert_eq!(effective["config"]["keep"], true);
        }
    }

    #[test]
    fn structural_replacement_conflicts_at_the_parent() {
        let applied = profile(AgentKind::Claude, r#"{"service":{"url":"a"}}"#, "{}", &[]);
        let working = ProfileDefinition {
            root: OverlayNode::Object(BTreeMap::from([(
                "config".to_string(),
                OverlayNode::Object(BTreeMap::from([(
                    "service".to_string(),
                    OverlayNode::Value(json!(["working"])),
                )])),
            )])),
        };
        let source = profile(AgentKind::Claude, r#"{"service":"source"}"#, "{}", &[]);
        let result = reconcile(&applied, &working, &source, &BTreeMap::new()).unwrap();
        let change = only(&result.changes);
        assert_eq!(change.path.to_string(), "/config/service");
        assert_eq!(change.class, ChangeClass::Conflict);
    }
}
