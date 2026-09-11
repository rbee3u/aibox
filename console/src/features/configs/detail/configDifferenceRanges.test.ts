import { describe, expect, it } from "vitest";
import { jsonPathRange } from "@/features/configs/detail/configDifferenceRanges";

describe("JSON difference locations", () => {
  it("locates escaped keys and multiline values after non-ASCII text", () => {
    const text = '{"😀":"前置", "a\\u002eb": {"token":\n "different"}}';
    const range = jsonPathRange(text, ["a.b", "token"]);
    expect(range && text.slice(...range)).toBe('"different"');
    expect(jsonPathRange(text, ["a", "b"])).toBeNull();
  });
  it("does not invent ranges for missing properties and locates blocking parents", () => {
    expect(jsonPathRange('{"env":{}}', ["env", "key"])).toBeNull();
    const text = '{"env": false}';
    const range = jsonPathRange(text, ["env", "key"]);
    expect(range && text.slice(...range)).toBe("false");
  });
});
