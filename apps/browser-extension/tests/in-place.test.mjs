import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(new URL("../src/55-in-place.js", import.meta.url), "utf8");
function mockTurn(id, type) {
  return {
    id,
    isConnected: true,
    getAttribute(key) { return key === "data-turn-type" ? type : ""; },
    querySelector() { return { click() {} }; }
  };
}
function context(turns) {
  let preview;
  let confirmationCalls = 0;
  const ctx = {
    document: { querySelectorAll: selector => selector === "ms-chat-turn" ? turns : [] },
    Branching: { getCurrentPromptId() { return "chat-test"; } },
    PromptPanel: { showCompactionReview(p) { preview = p; }, status() {} },
    state: { branchInProgress: false },
    fetchResolvedPromptThread: async () => ({ entries: [] }),
    extractResolvedTurns: () => [],
    assertSupportedEntries() {},
    confirm() { confirmationCalls++; return false; },
    setTimeout,
    Promise
  };
  const compactor = vm.runInNewContext(source + "\nInPlaceCompaction", ctx);
  return { compactor, get preview() { return preview; }, get confirmationCalls() { return confirmationCalls; } };
}
test("keeps the newest exchange and snapshot while selecting old thoughts for deletion", () => {
  const turns = [
    mockTurn("u1", "user"), mockTurn("t1", "thought"), mockTurn("m1", "text"),
    mockTurn("u2", "user"), mockTurn("m2", "model"),
    mockTurn("summary-prompt", "user"), mockTurn("summary-model", "model")
  ];
  const x = context(turns);
  const stage = x.compactor.stage("Remember these events", {
    totalRounds: 2, olderRounds: 1, recentRounds: 1
  });
  assert.equal(x.preview.snapshot, "Remember these events");
  const selection = x.compactor.preflightDom(stage);
  assert.deepEqual(Array.from(selection.targetIds), ["m1", "t1", "u1"]);
  assert.equal(selection.groups, 3);
  assert.equal(selection.items, 3);
  assert.equal(x.confirmationCalls, 0);
});
test("never deletes when older messages are not fully rendered", () => {
  const x = context([mockTurn("u1","user"),mockTurn("m1","model")]);
  assert.throws(() => x.compactor.preflightDom({
    totalRounds: 2, olderRounds: 1, recentRounds: 1
  }), /未完整加载/);
});
test("unknown role or incomplete exchange is rejected", () => {
  const x = context([]);
  assert.throws(() => x.compactor.collectGroups([mockTurn("u", "user"),mockTurn("x", "other")]), /未识别/);
  assert.throws(() => x.compactor.collectGroups([mockTurn("u", "user")]), /未完成/);
});
test("cancel clears staged deletion without mutating chat history", () => {
  const x = context([]);
  x.compactor.stage("snapshot", { totalRounds: 12, olderRounds: 4, recentRounds: 8 });
  x.compactor.cancel();
  assert.equal(x.compactor.pending, null);
  assert.equal(x.preview, null);
  assert.equal(x.confirmationCalls, 0);
});

test("recognizes AI Studio user and model turns without invented data-turn-type attributes", () => {
  const user = { id: "userA", getAttribute() { return ""; }, querySelector(sel) {
    if (sel === ".chat-turn-container") return { classList: { contains: name => name === "user" } };
    return {};
  }, classList: { contains: () => false } };
  const thought = { id: "thoughtA", getAttribute() { return ""; }, querySelector(sel) {
    if (sel === ".chat-turn-container") return null;
    if (sel === "ms-thought-chunk, .thought-panel") return {};
    if (sel === ".turn-content ms-text-chunk, .turn-content ms-code-block, .turn-content .cmark-node") return null;
    return {};
  }, classList: { contains: name => name === "thought-activity-host" } };
  const model = { id: "modelA", getAttribute() { return ""; }, querySelector(sel) {
    if (sel === ".chat-turn-container") return { classList: { contains: name => name === "render" } };
    return {};
  }, classList: { contains: () => false } };
  const x = context([]);
  const grouped = x.compactor.collectGroups([user, thought, model]);
  assert.equal(grouped.length, 1);
  assert.equal(grouped[0].turns.length, 3);
});
