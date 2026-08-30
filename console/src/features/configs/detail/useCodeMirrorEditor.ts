import { indentWithTab } from "@codemirror/commands";
import { json } from "@codemirror/lang-json";
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from "@codemirror/language";
import { toml } from "@codemirror/legacy-modes/mode/toml";
import { lintGutter, setDiagnostics } from "@codemirror/lint";
import { keymap } from "@codemirror/view";
import { basicSetup, EditorView } from "codemirror";
import { tags } from "@lezer/highlight";
import { useEffect, useRef } from "react";

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
  onChange,
}: CodeMirrorEditorOptions) {
  const parent = useRef<HTMLDivElement | null>(null);
  const view = useRef<EditorView | null>(null);
  const generation = useRef(0);

  useEffect(() => {
    if (!enabled || !parent.current) return;
    const language = file.endsWith(".json") ? json() : StreamLanguage.define(toml);
    const instance = new EditorView({
      parent: parent.current,
      doc: text,
      extensions: [
        basicSetup,
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

  return { parentRef: parent };
}
