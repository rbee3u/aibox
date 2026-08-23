import { render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { SessionMessageContent } from "./SessionMessageContent";
it("renders Agent Markdown safely and keeps user messages as plain text", () => {
  render(
    <>
      <SessionMessageContent
        role="assistant"
        text={
          '## Result\n\n`done`\n\n<script>alert("unsafe")</script>\n\n[docs](https://example.test)'
        }
      />
      <SessionMessageContent role="user" text={"**literal**\n<script>keep this text</script>"} />
    </>,
  );
  expect(screen.getByRole("heading", { name: "Result" })).toBeInTheDocument();
  expect(screen.getByText("done")).toBeInTheDocument();
  const docs = screen.getByRole("link", { name: "docs" });
  expect(docs).toHaveAttribute("href", "https://example.test");
  expect(docs).toHaveAttribute("target", "_blank");
  expect(docs).toHaveAttribute("rel", "noreferrer");
  expect(screen.queryByText(/alert\("unsafe"\)/)).not.toBeInTheDocument();
  expect(screen.getByText((content) => content.includes("**literal**"))).toHaveTextContent(
    "<script>keep this text</script>",
  );
  expect(document.querySelector("script")).toBeNull();
});
it("keeps non-HTTP Markdown destinations away from the Request Proxy", () => {
  render(
    <SessionMessageContent
      role="assistant"
      text={
        "[local file](/workspace/config.toml)\n\n[relative file](docs/console.md)\n\n[email](mailto:user@example.test)"
      }
    />,
  );
  expect(screen.queryByRole("link")).not.toBeInTheDocument();
  expect(screen.getByText("local file")).toHaveAttribute("title", "/workspace/config.toml");
  expect(screen.getByText("relative file")).toHaveAttribute("title", "docs/console.md");
  expect(screen.getByText("email")).toHaveAttribute("title", "mailto:user@example.test");
  expect(screen.getAllByTitle(/.+/).every((element) => element.tagName === "CODE")).toBe(true);
});
it("offers a copy button for fenced code blocks", () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  render(<SessionMessageContent role="assistant" text={"```ts\nconst answer = 42;\n```"} />);
  const copy = screen.getByRole("button", { name: "Copy code" });
  copy.click();
  expect(writeText).toHaveBeenCalledWith("const answer = 42;");
});
