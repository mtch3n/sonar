import { describe, expect, it } from "vitest";
import { hint, withValue } from "./pluginSettings";
import type { PluginSettings, Setting } from "./types";

const first: Setting = {
  key: "first",
  title: "Listed first",
  description: "The engine Enter searches",
  type: "choice",
  default: "google",
  options: [
    { value: "google", title: "Google" },
    { value: "duckduckgo", title: "DuckDuckGo" },
  ],
};

const number = (min: number | null, max: number | null): Setting => ({
  key: "results",
  title: "Results",
  description: null,
  type: "number",
  default: 3,
  min,
  max,
});

describe("withValue", () => {
  const own: PluginSettings = { enabled: true, keyword: "w" };

  it("keeps a changed value next to the plugin's own keys", () => {
    expect(withValue(own, first, "duckduckgo")).toEqual({ enabled: true, keyword: "w", first: "duckduckgo" });
  });

  it("leaves out a value set back to its default", () => {
    const changed = withValue(own, first, "duckduckgo");
    expect(withValue(changed, first, "google")).toEqual({ enabled: true, keyword: "w" });
  });

  it("compares by type, so 0 isn't false", () => {
    const toggle: Setting = { key: "safe", title: "Safe", description: null, type: "toggle", default: false };
    expect(withValue(own, toggle, true)).toMatchObject({ safe: true });
    expect(withValue({ ...own, safe: true }, toggle, false)).toEqual(own);
    expect(withValue(own, number(null, null), 0)).toMatchObject({ results: 0 });
  });
});

describe("hint", () => {
  it("is the description", () => {
    expect(hint(first)).toBe("The engine Enter searches");
  });

  it("adds a number's range", () => {
    expect(hint(number(1, 10))).toBe("1 to 10");
    expect(hint(number(1, null))).toBe("1 or more");
    expect(hint(number(null, 10))).toBe("10 or less");
    expect(hint(number(null, null))).toBeUndefined();
    expect(hint({ ...number(0, 5), description: "How many" })).toBe("How many · 0 to 5");
  });
});
