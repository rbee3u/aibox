import { readFile, unlink, writeFile } from "node:fs/promises";

const generatedIndex = new URL("../../../assets/index.html", import.meta.url);
const embeddedHtml = new URL("../../../assets/traffic.html", import.meta.url);
const html = (await readFile(generatedIndex, "utf8"))
  .replace("/_aibox/traffic/traffic.js", "/_aibox/traffic/app.js")
  .replace("/_aibox/traffic/traffic.css", "/_aibox/traffic/app.css");

await writeFile(embeddedHtml, html);
await unlink(generatedIndex);
