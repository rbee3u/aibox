import { readFile, unlink, writeFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

const outputDirectory = resolve(
  import.meta.dirname,
  globalThis.process.env.AIBOX_CONSOLE_OUT_DIR ?? "../../assets",
);
const generatedIndex = pathToFileURL(resolve(outputDirectory, "index.html"));
const embeddedHtml = pathToFileURL(resolve(outputDirectory, "console.html"));

function assertExactlyOnce(content, value) {
  const first = content.indexOf(value);
  if (first === -1 || content.indexOf(value, first + value.length) !== -1) {
    throw new Error(`expected generated HTML to contain ${JSON.stringify(value)} exactly once`);
  }
}

function replaceExactlyOnce(content, from, to) {
  assertExactlyOnce(content, from);
  return content.replace(from, to);
}

let html = await readFile(generatedIndex, "utf8");
html = replaceExactlyOnce(html, "/_aibox/ui/console.js", "/_aibox/ui/app.js");
html = replaceExactlyOnce(html, "/_aibox/ui/console.css", "/_aibox/ui/app.css");
assertExactlyOnce(html, "__AIBOX_CSP_NONCE__");

await writeFile(embeddedHtml, html);
await unlink(generatedIndex);
