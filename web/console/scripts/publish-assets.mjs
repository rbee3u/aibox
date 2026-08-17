import { readFile, unlink, writeFile } from "node:fs/promises";
import { URL } from "node:url";

const generatedIndex = new URL("../../../assets/index.html", import.meta.url);
const embeddedHtml = new URL("../../../assets/console.html", import.meta.url);

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

await writeFile(embeddedHtml, html);
await unlink(generatedIndex);
