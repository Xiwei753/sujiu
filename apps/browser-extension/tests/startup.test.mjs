import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const code = await readFile(new URL("../src/90-bootstrap.js", import.meta.url), "utf8");

function start({ inAppContext = false, networkFails = false } = {}) {
  const calls = [];
  const win = {};
  const ctx = {
    window: win,
    location: { href: "https://aistudio.google.com/prompts/new_chat" },
    SCRIPT_VERSION: "0.1.1",
    Config: { TAGS: { UI: "test" } },
    env: { isAppContext: inAppContext },
    PromptPanel: { mount() { calls.push("panel"); } },
    NetworkMonitor: { init() { calls.push("network"); if (networkFails) throw new Error("network failure"); } },
    installTopLevelHandlers() { calls.push("events"); },
    UI: { init() { calls.push("toolbar"); } },
    console: { info() {}, error() {} }
  };
  vm.runInNewContext(code, ctx);
  return { calls, info: win.__SUJIU_AISTUDIO_RUNTIME__ };
}

test("UI mounts before network interceptors and survives an interceptor failure", () => {
  const result = start({ networkFails: true });
  assert.equal(result.calls[0], "panel");
  assert.equal(result.info.panelInitialized, true);
  assert.equal(result.info.networkInitialized, false);
  assert.equal(result.info.errors[0].stage, "network");
  assert.ok(result.calls.includes("toolbar"));
});

test("panel also mounts in legacy /app routes where native toolbar is skipped", () => {
  const result = start({ inAppContext: true });
  assert.equal(result.info.panelInitialized, true);
  assert.ok(result.calls.includes("panel"));
  assert.ok(!result.calls.includes("toolbar"));
});
