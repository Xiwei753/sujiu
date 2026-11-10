import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(new URL("../src/30-compression.js", import.meta.url), "utf8");
const networkSource = await readFile(new URL("../src/20-network.js", import.meta.url), "utf8");

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

test("failed Google response releases both compression lifecycle flags", () => {
  const state={ compressionInProgress:true, compressionRequestPending:true,
    compressionResponsePending:true, historyCapturePending:true };
  const messages=[];
  const ctx={
    state, UI:{setButtonLoading(){}},
    PromptPanel:{status(message){messages.push(message)}},
    log:{net(){}},
    isTargetRequest:()=>true
  };
  const compression=vm.runInNewContext(source+"\nCompression",ctx);
  compression.handleResponseEvent({detail:{url:"GenerateContent",status:500,bodyText:"Unknown tool"}});
  assert.equal(state.compressionInProgress,false);
  assert.equal(state.compressionRequestPending,false);
  assert.equal(state.compressionResponsePending,false);
  assert.equal(state.historyCapturePending,false);
  assert.match(messages.join(" "),/错误 500/);
});

test("malformed tool response releases compression lifecycle flags", () => {
  const state={ compressionInProgress:true,compressionRequestPending:true,
    compressionResponsePending:true,historyCapturePending:true };
  const messages=[];
  const ctx={
    state, UI:{setButtonLoading(){}},
    PromptPanel:{status(message){messages.push(message)}},
    log:{net(){}},
    isTargetRequest:()=>true,
    safeParseJSON:()=>({}),
    extractStateSnapshot:()=>{throw new Error("Unknown search tool")}
  };
  const compression=vm.runInNewContext(source+"\nCompression",ctx);
  compression.handleResponseEvent({detail:{url:"GenerateContent",status:200,bodyText:"{}"}});
  assert.equal(state.compressionInProgress,false);
  assert.equal(state.compressionResponsePending,false);
  assert.match(messages.join(" "),/Unknown search tool/);
});

test("network failures and aborted GenerateContent calls reset compression lock", () => {
  assert.match(networkSource,/catch \(error\) \{\s*if \(shouldLogGenerate && state.compressionInProgress\) \{/);
  assert.match(networkSource,/for \(const eventName of \["error", "abort", "timeout"\]\)/);
  assert.match(networkSource,/Compression.finalizeRun\(\)/);
});

