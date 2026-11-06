import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import { readFile } from "node:fs/promises";

const source = await readFile(new URL("../src/67-save-monitor.js", import.meta.url), "utf8");
const net = await readFile(new URL("../src/20-network.js", import.meta.url), "utf8");
test("observes native save status without serializing any request body", () => {
  const runtime = {};
  const monitor = vm.runInNewContext(source + "\nSaveMonitor", {
    window: { __SUJIU_AISTUDIO_RUNTIME__: runtime },
    isTargetRequest: (url, key) => key === "UPDATE" && url.endsWith("/UpdatePrompt"),
    log: { net() {} }
  });
  monitor.record(200, "https://example.test/UpdatePrompt");
  monitor.record(429, "https://example.test/UpdatePrompt");
  monitor.record(200, "https://example.test/GenerateContent");
  assert.equal(monitor.attempts, 2);
  assert.equal(runtime.save.successes, 1);
  assert.equal(runtime.save.status, 429);
  assert.equal(runtime.save.ok, false);
  assert.deepEqual(Object.keys(runtime.save).sort(), ["attempts","ok","status","successes","time"]);
});
test("XHR and fetch both pass native UpdatePrompt response status into observer", () => {
  assert.match(net, /SaveMonitor\.record\(result\.status,\s*url\)/);
  assert.match(net, /SaveMonitor\.record\(realXHR\.status,\s*responseUrl\)/);
});


test("waits for the existing native autosave, without submitting a new write", async () => {
  const runtime = {};
  const monitor = vm.runInNewContext(source + "\nSaveMonitor", {
    window: { __SUJIU_AISTUDIO_RUNTIME__: runtime },
    isTargetRequest: (url, key) => key === "UPDATE" && url.endsWith("/UpdatePrompt"),
    log: { net() {} },
    setTimeout
  });
  const before = monitor.attempts;
  const lastDeletionStartedAt = Date.now();
  const pending = monitor.waitForNativeSave(before, lastDeletionStartedAt, 30, 800);
  setTimeout(() => monitor.record(200, "https://example.test/UpdatePrompt"), 20);
  const result = await pending;
  assert.equal(result.observed, true);
  assert.equal(result.ok, true);
  assert.equal(result.status, 200);
  assert.equal(result.attempts, 1);
});

test("an older successful UpdatePrompt response never counts as the final deletion save", async () => {
  const monitor = vm.runInNewContext(source + "\nSaveMonitor", {
    window: { __SUJIU_AISTUDIO_RUNTIME__: {} },
    isTargetRequest: (url, key) => key === "UPDATE" && url.endsWith("/UpdatePrompt"),
    log: { net() {} },
    setTimeout
  });
  monitor.record(200, "https://example.test/UpdatePrompt");
  const later = Date.now() + 2000;
  const result = await monitor.waitForNativeSave(0, later, 10, 150);
  assert.equal(result.observed, false);
  assert.equal(result.ok, false);
});
