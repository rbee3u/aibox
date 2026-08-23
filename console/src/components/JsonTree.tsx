import { Check, ChevronDown, ChevronRight, Clipboard } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import {
  isJsonContainer,
  jsonEntries,
  jsonStringPreview,
  jsonValueType,
  stringifyJson,
  type JsonValue,
} from "../bodyPresentation";
import { useClipboardFeedback } from "../useClipboardFeedback";
import styles from "./JsonTree.module.css";

interface JsonTreeProps {
  value: JsonValue;
  compact?: boolean;
  pathPrefix?: string;
  expanded: Set<string>;
  expandedStrings: Set<string>;
  onToggle: (path: string) => void;
  onToggleString: (path: string) => void;
}

export function JsonTree({
  value,
  compact = false,
  pathPrefix = "$",
  expanded,
  expandedStrings,
  onToggle,
  onToggleString,
}: JsonTreeProps) {
  const [copiedPath, copyValue] = useClipboardFeedback<string>();
  const [activePath, setActivePath] = useState(pathPrefix);
  const nodeRefs = useRef(new Map<string, HTMLDivElement>());
  const visibleNodes = useMemo(
    () => collectVisibleNodes(value, pathPrefix, expanded),
    [expanded, pathPrefix, value],
  );
  const resolvedActivePath = visibleNodes.some((node) => node.path === activePath)
    ? activePath
    : pathPrefix;

  function copy(path: string, node: JsonValue) {
    const text = isJsonContainer(node) ? stringifyJson(node, true) : stringifyJson(node);
    void copyValue(text, path);
  }

  function focusNode(path: string) {
    setActivePath(path);
    nodeRefs.current.get(path)?.focus();
  }

  function navigateTree(event: KeyboardEvent<HTMLDivElement>, path: string) {
    if (event.target !== event.currentTarget) return;
    const index = visibleNodes.findIndex((node) => node.path === path);
    const current = visibleNodes[index];
    if (!current) return;
    let destination: string | null = null;
    switch (event.key) {
      case "ArrowDown":
        destination = visibleNodes[index + 1]?.path ?? null;
        break;
      case "ArrowUp":
        destination = visibleNodes[index - 1]?.path ?? null;
        break;
      case "Home":
        destination = visibleNodes[0]?.path ?? null;
        break;
      case "End":
        destination = visibleNodes.at(-1)?.path ?? null;
        break;
      case "ArrowRight": {
        if (!current.container) break;
        if (!current.open) {
          onToggle(path);
        } else {
          const next = visibleNodes[index + 1];
          destination = next?.parentPath === path ? next.path : null;
        }
        break;
      }
      case "ArrowLeft":
        if (current.container && current.open && path !== pathPrefix) onToggle(path);
        else destination = current.parentPath;
        break;
      case "Enter":
      case " ":
        if (current.container && path !== pathPrefix) onToggle(path);
        break;
      default:
        return;
    }
    event.preventDefault();
    if (destination) focusNode(destination);
  }

  return (
    <div
      className={`${styles.jsonTree} ${compact ? styles.compact : ""}`}
      role="tree"
      aria-label="JSON body"
    >
      <JsonNode
        value={value}
        path={pathPrefix}
        depth={0}
        expanded={expanded}
        expandedStrings={expandedStrings}
        copiedPath={copiedPath}
        activePath={resolvedActivePath}
        registerNode={(path, element) => {
          if (element) nodeRefs.current.set(path, element);
          else nodeRefs.current.delete(path);
        }}
        onNodeFocus={setActivePath}
        onNodeKeyDown={navigateTree}
        onToggle={onToggle}
        onToggleString={onToggleString}
        onCopy={copy}
      />
    </div>
  );
}

interface JsonNodeProps {
  value: JsonValue;
  path: string;
  depth: number;
  name?: string;
  expanded: Set<string>;
  expandedStrings: Set<string>;
  copiedPath: string | null;
  activePath: string;
  registerNode: (path: string, element: HTMLDivElement | null) => void;
  onNodeFocus: (path: string) => void;
  onNodeKeyDown: (event: KeyboardEvent<HTMLDivElement>, path: string) => void;
  onToggle: (path: string) => void;
  onToggleString: (path: string) => void;
  onCopy: (path: string, value: JsonValue) => void;
}

function JsonNode(props: JsonNodeProps) {
  const { value, path, depth, name, expanded, expandedStrings, copiedPath } = props;
  const container = isJsonContainer(value);
  const entries = container ? jsonEntries(value) : [];
  const open = depth === 0 || expanded.has(path);
  const stringPreview = typeof value === "string" ? jsonStringPreview(value) : null;
  const stringOpen = expandedStrings.has(path);

  return (
    <div
      ref={(element) => props.registerNode(path, element)}
      className={styles.jsonNode}
      role="treeitem"
      aria-expanded={container ? open : undefined}
      aria-level={depth + 1}
      tabIndex={props.activePath === path ? 0 : -1}
      onFocus={(event) => event.target === event.currentTarget && props.onNodeFocus(path)}
      onKeyDown={(event) => props.onNodeKeyDown(event, path)}
    >
      <div className={styles.jsonLine}>
        {container ? (
          <button
            type="button"
            className={styles.jsonToggle}
            onClick={() => depth > 0 && props.onToggle(path)}
            disabled={depth === 0}
            tabIndex={-1}
            aria-label={`${open ? "Collapse" : "Expand"} ${name ?? "JSON root"}`}
            aria-expanded={open}
          >
            {open ? (
              <ChevronDown size={14} aria-hidden="true" />
            ) : (
              <ChevronRight size={14} aria-hidden="true" />
            )}
          </button>
        ) : (
          <span className={styles.jsonIndent} aria-hidden="true" />
        )}
        {name !== undefined && <span className={styles.jsonKey}>{JSON.stringify(name)}:</span>}
        {container ? (
          <span className={styles.jsonContainerLabel}>
            {Array.isArray(value) ? "Array" : "Object"} · {entries.length}{" "}
            {entries.length === 1 ? "item" : "items"}
          </span>
        ) : (
          <JsonScalar value={value} preview={stringOpen ? null : stringPreview} />
        )}
        {stringPreview !== null && (
          <button
            type="button"
            className={styles.jsonStringToggle}
            onClick={() => props.onToggleString(path)}
          >
            {stringOpen ? "Show less" : "Show all"}
          </button>
        )}
        <button
          type="button"
          className={styles.jsonCopy}
          onClick={() => props.onCopy(path, value)}
          aria-label={
            copiedPath === path ? "JSON value copied" : `Copy ${jsonValueType(value)} value`
          }
          title={copiedPath === path ? "JSON value copied" : "Copy JSON value"}
        >
          {copiedPath === path ? (
            <Check size={13} aria-hidden="true" />
          ) : (
            <Clipboard size={13} aria-hidden="true" />
          )}
        </button>
      </div>
      {container && open && (
        <div className={styles.jsonChildren} role="group">
          {entries.map(([key, child]) => (
            <JsonNode
              {...props}
              key={key}
              value={child}
              name={key}
              path={`${path}/${escapePath(key)}`}
              depth={depth + 1}
            />
          ))}
        </div>
      )}
    </div>
  );
}

interface VisibleJsonNode {
  path: string;
  parentPath: string | null;
  container: boolean;
  open: boolean;
}

function collectVisibleNodes(
  value: JsonValue,
  path: string,
  expanded: Set<string>,
  parentPath: string | null = null,
  depth = 0,
): VisibleJsonNode[] {
  const container = isJsonContainer(value);
  const open = container && (depth === 0 || expanded.has(path));
  const nodes: VisibleJsonNode[] = [{ path, parentPath, container, open }];
  if (!open) return nodes;
  for (const [key, child] of jsonEntries(value)) {
    const childPath = `${path}/${escapePath(key)}`;
    nodes.push(...collectVisibleNodes(child, childPath, expanded, path, depth + 1));
  }
  return nodes;
}

function JsonScalar({ value, preview }: { value: JsonValue; preview: string | null }) {
  const rendered = preview === null ? stringifyJson(value) : `${JSON.stringify(preview)}…`;
  return <span className={styles[`json${capitalize(jsonValueType(value))}`]}>{rendered}</span>;
}

function escapePath(value: string): string {
  return value.replaceAll("~", "~0").replaceAll("/", "~1");
}

function capitalize(value: string): string {
  return `${value[0].toUpperCase()}${value.slice(1)}`;
}
