import { gzipSync } from "node:zlib";
import { readFile } from "node:fs/promises";
import { URL } from "node:url";

const baselineBytes = 369_891;
const maxGrowthBytes = 65_536;
const asset = new URL("../../assets/console.js", import.meta.url);
const content = await readFile(asset);
const compressedBytes = gzipSync(content, { level: 9, mtime: 0 }).byteLength;
const maximumBytes = baselineBytes + maxGrowthBytes;

if (compressedBytes > maximumBytes) {
  throw new Error(
    `console.js gzip size ${compressedBytes} exceeds the ${maximumBytes}-byte budget ` +
      `(${maxGrowthBytes} bytes above the ${baselineBytes}-byte baseline)`,
  );
}

globalThis.process.stdout.write(
  `console.js gzip size: ${compressedBytes} bytes ` +
    `(baseline ${baselineBytes}, growth ${compressedBytes - baselineBytes})\n`,
);
