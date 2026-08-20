import { Check, Copy } from "lucide-react";
import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import { Children, isValidElement } from "react";
import type { ComponentPropsWithoutRef, ReactNode } from "react";
import { useClipboardFeedback } from "../useClipboardFeedback";
import styles from "./SessionMessageContent.module.css";

interface SessionMessageContentProps {
  role: "user" | "assistant";
  text: string;
}

type MarkdownCodeProps = ComponentPropsWithoutRef<"code"> & { node?: unknown };

function withoutNode<T extends { node?: unknown }>(props: T): Omit<T, "node"> {
  const next = { ...props };
  delete next.node;
  return next;
}

function textFromChildren(value: ReactNode): string {
  return Children.toArray(value)
    .map((child) => {
      if (typeof child === "string" || typeof child === "number") return String(child);
      if (isValidElement<{ children?: ReactNode }>(child)) {
        return textFromChildren(child.props.children);
      }
      return "";
    })
    .join("");
}

function MarkdownCode({ className, children, ...props }: MarkdownCodeProps) {
  const source = textFromChildren(children).replace(/\n$/, "");
  const inline = !className && !source.includes("\n");
  const [copied, copy] = useClipboardFeedback();
  const codeProps = withoutNode(props);

  if (inline) {
    return (
      <code className={className} {...codeProps}>
        {children}
      </code>
    );
  }

  const language = className?.replace(/^language-/, "");
  return (
    <span className={styles.codeBlock}>
      <span className={styles.codeToolbar}>
        <span>{language || "Code"}</span>
        <button
          type="button"
          className={styles.copyCode}
          aria-label={copied ? "Code copied" : "Copy code"}
          title={copied ? "Code copied" : "Copy code"}
          onClick={() => void copy(source, true)}
        >
          {copied ? <Check size={13} aria-hidden="true" /> : <Copy size={13} aria-hidden="true" />}
        </button>
      </span>
      <pre>
        <code className={className} {...codeProps}>
          {children}
        </code>
      </pre>
    </span>
  );
}

function PlainMessage({ text }: { text: string }) {
  return <pre className={styles.plainText}>{text}</pre>;
}

function MarkdownMessage({ text }: { text: string }) {
  return (
    <div className={styles.markdown}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
        skipHtml
        components={{
          a: (props) => {
            const anchorProps = withoutNode(props);
            return <a {...anchorProps} target="_blank" rel="noreferrer" />;
          },
          code: MarkdownCode,
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}

export function SessionMessageContent({ role, text }: SessionMessageContentProps) {
  return role === "assistant" ? <MarkdownMessage text={text} /> : <PlainMessage text={text} />;
}

export function SessionMessageRole({ children }: { children: ReactNode }) {
  return <span className={styles.role}>{children}</span>;
}
