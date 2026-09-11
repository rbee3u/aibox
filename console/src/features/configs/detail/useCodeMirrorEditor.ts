import { indentWithTab } from "@codemirror/commands";
import { json } from "@codemirror/lang-json";
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from "@codemirror/language";
import { toml } from "@codemirror/legacy-modes/mode/toml";
import { lintGutter, setDiagnostics } from "@codemirror/lint";
import { Decoration, GutterMarker, gutter, keymap } from "@codemirror/view";
import { Compartment, RangeSet, StateField } from "@codemirror/state";
import { basicSetup, EditorView } from "codemirror";
import { tags } from "@lezer/highlight";
import { useCallback, useEffect, useRef } from "react";

export interface RawDiagnostic {
  message: string;
  line: number;
  column: number;
}

const configHighlightStyle = HighlightStyle.define([
  { tag: tags.propertyName, class: "cm-config-key" },
  { tag: tags.string, class: "cm-config-string" },
  { tag: tags.number, class: "cm-config-number" },
  { tag: [tags.bool, tags.null, tags.atom], class: "cm-config-boolean" },
  { tag: tags.comment, class: "cm-config-comment" },
  { tag: tags.invalid, class: "cm-config-invalid" },
]);

/**
 * CodeMirror generates its own style elements, so the embedded Console's
 * Content Security Policy requires the request-scoped nonce.
 */
function codeMirrorCspNonce(): string {
  return document.querySelector<HTMLMetaElement>('meta[name="aibox-csp-nonce"]')?.content ?? "";
}

/** jsdom cannot host CodeMirror, so tests fall back to a plain text area. */
export const codeMirrorAvailable =
  typeof navigator === "undefined" || !/jsdom/i.test(navigator.userAgent);

interface CodeMirrorEditorOptions {
  /** The editor mounts only while the Raw view of an editable file is active. */
  enabled: boolean;
  file: string;
  document: string;
  diagnostics: RawDiagnostic[];
  differences: Array<{ range: [number, number]; path: string[] }>;
  onDifference: (path: string[]) => void;
  onChange: (value: string) => void;
}

/**
 * Owns one CodeMirror instance for a Config file. The instance survives
 * document changes; only a different file, mode, or editability rebuilds it.
 */
export function useCodeMirrorEditor({
  enabled,
  file,
  document: text,
  diagnostics,
  differences,
  onDifference,
  onChange,
}: CodeMirrorEditorOptions) {
  const parent = useRef<HTMLDivElement | null>(null);
  const view = useRef<EditorView | null>(null);
  const generation = useRef(0);
  const comparison = useRef(new Compartment());

  useEffect(() => {
    if (!enabled || !parent.current) return;
    const language = file.endsWith(".json") ? json() : StreamLanguage.define(toml);
    const instance = new EditorView({
      parent: parent.current,
      doc: text,
      extensions: [
        basicSetup,
        comparison.current.of([]),
        // Keep long configuration lines readable within the editor pane.
        // This avoids a nested horizontal scrollbar on desktop and narrow views.
        EditorView.lineWrapping,
        language,
        EditorView.cspNonce.of(codeMirrorCspNonce()),
        syntaxHighlighting(configHighlightStyle),
        lintGutter(),
        keymap.of([indentWithTab]),
        EditorView.contentAttributes.of({ "aria-label": `${file} content` }),
        EditorView.updateListener.of((update) => {
          if (!update.docChanged) return;
          onChange(update.state.doc.toString());
        }),
      ],
    });
    view.current = instance;
    return () => {
      generation.current += 1;
      instance.destroy();
      view.current = null;
    };
    // The document is synchronized by the next effect rather than remounting.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, file, onChange]);

  useEffect(() => {
    const instance = view.current;
    if (!instance) return;
    if (instance.state.doc.toString() !== text) {
      instance.dispatch({ changes: { from: 0, to: instance.state.doc.length, insert: text } });
    }
  }, [text]);

  useEffect(() => {
    const instance = view.current;
    if (!instance) return;
    instance.dispatch(
      setDiagnostics(
        instance.state,
        diagnostics.map((diagnostic) => {
          const lineInfo = instance.state.doc.line(
            Math.min(Math.max(1, diagnostic.line), instance.state.doc.lines),
          );
          const from = Math.min(lineInfo.from + Math.max(1, diagnostic.column) - 1, lineInfo.to);
          return {
            from,
            to: Math.min(from + 1, lineInfo.to),
            severity: "error" as const,
            message: diagnostic.message,
          };
        }),
      ),
    );
  }, [diagnostics]);

  useEffect(() => {
    const instance = view.current;
    if (!instance) return;
    const lines = new Map<number, string[]>();
    const highlights = new Set<number>();
    for (const {
      range: [from, to],
      path,
    } of differences) {
      if (from < 0 || to > instance.state.doc.length) continue;
      const first = instance.state.doc.lineAt(from);
      const last = instance.state.doc.lineAt(Math.max(from, to - 1));
      if (!lines.has(first.from)) lines.set(first.from, path);
      for (let number = first.number; number <= last.number; number++)
        highlights.add(instance.state.doc.line(number).from);
    }
    class DifferenceMarker extends GutterMarker {
      constructor(readonly path: string[]) {
        super();
      }
      toDOM() {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "cm-config-difference-button";
        button.textContent = "≠";
        button.setAttribute("aria-label", `Show difference for ${this.path.join(".")}`);
        button.addEventListener("click", () => onDifference(this.path));
        return button;
      }
    }
    const markers = RangeSet.of(
      [...lines]
        .sort(([a], [b]) => a - b)
        .map(([position, path]) => new DifferenceMarker(path).range(position)),
    );
    const decorations = Decoration.set(
      [...highlights]
        .sort((a, b) => a - b)
        .map((position) => Decoration.line({ class: "cm-config-difference-line" }).range(position)),
    );
    const highlightField = StateField.define({
      create: () => decorations,
      update: (value, transaction) => (transaction.docChanged ? Decoration.none : value),
      provide: (field) => EditorView.decorations.from(field),
    });
    const markerField = StateField.define({
      create: () => markers,
      update: (value, transaction) =>
        transaction.docChanged ? (RangeSet.empty as typeof markers) : value,
    });
    instance.dispatch({
      effects: comparison.current.reconfigure([
        highlightField,
        markerField,
        gutter({
          class: "cm-config-difference-gutter",
          markers: (editorView) => editorView.state.field(markerField),
        }),
      ]),
    });
    // CodeMirror hides decorative gutters from assistive technology by default.
    // Our gutter contains real controls; keep only the decorative siblings hidden.
    for (const gutters of instance.dom.querySelectorAll(".cm-gutters")) {
      gutters.removeAttribute("aria-hidden");
      for (const child of gutters.children) {
        if (child.classList.contains("cm-config-difference-gutter"))
          child.removeAttribute("aria-hidden");
        else child.setAttribute("aria-hidden", "true");
      }
    }
  }, [differences, onDifference, enabled]);

  const revealRange = useCallback((range: [number, number]) => {
    const instance = view.current;
    if (!instance || range[1] > instance.state.doc.length) return;
    instance.dispatch({
      selection: { anchor: range[0], head: range[1] },
      effects: EditorView.scrollIntoView(range[0], { y: "center" }),
    });
    instance.focus();
  }, []);
  return { parentRef: parent, revealRange };
}
