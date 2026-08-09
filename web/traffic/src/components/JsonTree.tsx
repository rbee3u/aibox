import { Check, ChevronDown, ChevronRight, Clipboard } from "lucide-react";
import { isLosslessNumber } from "lossless-json";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  isJsonContainer,
  jsonEntries,
  jsonValueType,
  LONG_STRING_CHARACTERS,
  shouldTruncateJsonString,
  stringifyJson,
  type JsonValue,
} from "../bodyPresentation";
import styles from "./RecordDetail.module.css";

interface JsonTreeProps {
  value: JsonValue;
  pathPrefix?: string;
  expanded: Set<string>;
  expandedStrings: Set<string>;
  onToggle: (path: string) => void;
  onToggleString: (path: string) => void;
}

export function JsonTree({
  value,
  pathPrefix = "$",
  expanded,
  expandedStrings,
  onToggle,
  onToggleString,
}: JsonTreeProps) {
  const [copiedPath, setCopiedPath] = useState<string | null>(null);
  const [activePath, setActivePath] = useState(pathPrefix);
  const copiedTimer = useRef<number | undefined>(undefined);
  const nodeRefs = useRef(new Map<string, HTMLDivElement>());
  const visibleNodes = useMemo(
    () => collectVisibleNodes(value, pathPrefix, expanded),
    [expanded, pathPrefix, value],
  );
  const resolvedActivePath = visibleNodes.some((node) => node.path === activePath)
    ? activePath
    : pathPrefix;

  useEffect(
    () => () => {
      if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
    },
    [],
  );

  async function copy(path: string, node: JsonValue) {
    try {
      await navigator.clipboard.writeText(
        isJsonContainer(node) ? stringifyJson(node, true) : stringifyJson(node),
      );
      setCopiedPath(path);
      if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
      copiedTimer.current = window.setTimeout(() => setCopiedPath(null), 1400);
    } catch {
      setCopiedPath(null);
    }
  }

  function focusNode(path: string) {
    setActivePath(path);
    nodeRefs.current.get(path)?.focus();
  }

  function navigateTree(event: React.KeyboardEvent<HTMLDivElement>, path: string) {
    if (event.target !== event.currentTarget) return;
    const index = visibleNodes.findIndex((node) => node.path === path);
    const current = visibleNodes[index];
    if (!current) return;
    let destination: string | null = null;
    if (event.key === "ArrowDown") destination = visibleNodes[index + 1]?.path ?? null;
    if (event.key === "ArrowUp") destination = visibleNodes[index - 1]?.path ?? null;
    if (event.key === "Home") destination = visibleNodes[0]?.path ?? null;
    if (event.key === "End") destination = visibleNodes.at(-1)?.path ?? null;
    if (event.key === "ArrowRight" && current.container) {
      if (!current.open) onToggle(path);
      else
        destination =
          visibleNodes[index + 1]?.parentPath === path ? visibleNodes[index + 1].path : null;
    }
    if (event.key === "ArrowLeft") {
      if (current.container && current.open && path !== pathPrefix) onToggle(path);
      else destination = current.parentPath;
    }
    if ((event.key === "Enter" || event.key === " ") && current.container && path !== pathPrefix) {
      onToggle(path);
    }
    if (
      destination === null &&
      !["ArrowDown", "ArrowUp", "ArrowRight", "ArrowLeft", "Home", "End", "Enter", " "].includes(
        event.key,
      )
    ) {
      return;
    }
    event.preventDefault();
    if (destination) focusNode(destination);
  }

  return (
    <div className={styles.jsonTree} role="tree" aria-label="JSON body">
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
        onCopy={(path, node) => void copy(path, node)}
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
  onNodeKeyDown: (event: React.KeyboardEvent<HTMLDivElement>, path: string) => void;
  onToggle: (path: string) => void;
  onToggleString: (path: string) => void;
  onCopy: (path: string, value: JsonValue) => void;
}

function JsonNode(props: JsonNodeProps) {
  const { value, path, depth, name, expanded, expandedStrings, copiedPath } = props;
  const container = isJsonContainer(value);
  const entries = container ? jsonEntries(value) : [];
  const open = depth === 0 || expanded.has(path);
  const longString = typeof value === "string" && shouldTruncateJsonString(value);
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
          <JsonScalar value={value} truncated={longString && !stringOpen} />
        )}
        {longString && (
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

function JsonScalar({ value, truncated }: { value: JsonValue; truncated: boolean }) {
  let rendered = stringifyJson(value);
  if (truncated && typeof value === "string") {
    rendered = `${JSON.stringify([...value].slice(0, LONG_STRING_CHARACTERS).join(""))}…`;
  }
  const type = isLosslessNumber(value) ? "number" : value === null ? "null" : typeof value;
  return <span className={styles[`json${capitalize(type)}`]}>{rendered}</span>;
}

function escapePath(value: string): string {
  return value.replaceAll("~", "~0").replaceAll("/", "~1");
}

function capitalize(value: string): string {
  return `${value[0].toUpperCase()}${value.slice(1)}`;
}
