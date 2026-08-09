import { Check, ChevronDown, ChevronRight, Clipboard } from "lucide-react";
import { isLosslessNumber } from "lossless-json";
import { useEffect, useRef, useState } from "react";
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
  const copiedTimer = useRef<number | undefined>(undefined);

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

  return (
    <div className={styles.jsonTree} role="tree" aria-label="JSON body">
      <JsonNode
        value={value}
        path={pathPrefix}
        depth={0}
        expanded={expanded}
        expandedStrings={expandedStrings}
        copiedPath={copiedPath}
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
    <div className={styles.jsonNode} role="treeitem" aria-expanded={container ? open : undefined}>
      <div className={styles.jsonLine}>
        {container ? (
          <button
            type="button"
            className={styles.jsonToggle}
            onClick={() => depth > 0 && props.onToggle(path)}
            disabled={depth === 0}
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
