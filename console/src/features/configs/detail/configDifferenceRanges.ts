import { jsonLanguage } from "@codemirror/lang-json";
import type { SyntaxNode } from "@lezer/common";
import type { ConfigDifference } from "@/api/configs";

/** Locate paths in this exact JSON text, without interpreting comparison semantics. */
export function jsonPathRange(text: string, path: readonly string[]): [number, number] | null {
  let node: SyntaxNode | null = jsonLanguage.parser.parse(text).topNode.firstChild;
  for (const segment of path) {
    if (!node) return null;
    if (node.name !== "Object") return [node.from, node.to];
    let found: SyntaxNode | null = null;
    for (let property = node.firstChild; property; property = property.nextSibling) {
      if (property.name !== "Property") continue;
      const name = property.getChild("PropertyName");
      if (!name || JSON.parse(text.slice(name.from, name.to)) !== segment) continue;
      found = property.lastChild;
      break;
    }
    node = found;
  }
  return node ? [node.from, node.to] : null;
}

export function differenceRange(
  file: string,
  text: string,
  difference: ConfigDifference,
  current: boolean,
): [number, number] | null {
  return file.endsWith(".json")
    ? jsonPathRange(text, difference.path)
    : current
      ? difference.current_range
      : difference.named_range;
}
