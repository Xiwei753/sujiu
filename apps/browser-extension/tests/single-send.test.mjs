import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(new URL("../src/30-compression.js", import.meta.url), "utf8");

function setup({ label = "Run", disabled = false, capture = true } = {}) {
  let clicks = 0;
  let messages = [];
  const state = { compressionInProgress: true, compressionRequestPending: true, compressionResponsePending: false };
  const button = {
    disabled,
    textContent: label,
    getAttribute(name) { return name === "aria-label" ? label : null; },
    querySelector() { return null; },
    click() {
      clicks++;
      if (capture) state.compressionRequestPending = false;
    }
  };
  const ctx = {
    state,
    UI: { findSendButton() { return button; } },
    PromptPanel: { status(s) { messages.push(s); } },
    log: { ui() {} },
    requestAnimationFrame(cb) { cb(0); },
    setTimeout,
    Date,
    Promise
  };
  const methods = vm.runInNewContext(source + "\n({sendCompressionRequest,isStopControl})", ctx);
  return { button, state, methods, get clicks() { return clicks; }, get messages() { return messages; } };
}

test("one click sends once and never retries after Run becomes Stop", async () => {
  const context = setup();
  const submitted = await context.methods.sendCompressionRequest();
  assert.equal(submitted, true);
  assert.equal(context.clicks, 1);
});

test("Stop / cancel controls never receive a click", async () => {
  for (const label of ["Stop", "Cancel", "停止生成"]) {
    const context = setup({ label });
    const submitted = await context.methods.sendCompressionRequest();
    assert.equal(submitted, false);
    assert.equal(context.clicks, 0);
  }
});

test("disabled Run is not forcibly enabled", async () => {
  const context = setup({ disabled: true });
  const submitted = await context.methods.sendCompressionRequest();
  assert.equal(submitted, false);
  assert.equal(context.clicks, 0);
  assert.equal(context.button.disabled, true);
});

test("no synthetic pointer chain, send retries, or forced enabling", () => {
  assert.doesNotMatch(source, /sendPromptWithRetries|dispatchPointerClickSequence|enableSendButton|triggerKeyboardSend/);
  assert.match(source, /button\.click\(\)/);
});
