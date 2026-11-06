import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(new URL("../src/55-in-place.js", import.meta.url), "utf8");
function mockTurn(id, type) {
  return {
    id,
    isConnected: true,
    textContent: id,
    getAttribute(key) { return key === "data-turn-type" ? type : ""; },
    querySelector(selector) { return selector.includes("ms-chat-turn-options") ? { click() {} } : null; },
    classList: { contains() { return false; } }
  };
}
function context(turns) {
  let preview;
  let confirmationCalls = 0;
  const ctx = {
    document: { querySelectorAll: selector => selector === "ms-chat-turn" ? turns : [] },
    Branching: { getCurrentPromptId() { return "chat-test"; } },
    CompactionTask: { isPinnedSummary: text => /\[\[SUJIU_MEMORY_V1:/.test(text), memoryMarker: marker => marker.replace("SCOPE_V2", "MEMORY_V1") },
    SaveMonitor: { attempts: 0, latest: null },
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

test("previously compressed summary rounds are pinned and never enter the deletion list", () => {
  const older = [
    {role: "user", text: "very old"},
    {role: "model", text: "old response"},
    {role: "user", text: "summarize"},
    {role: "model", text: "[[SUJIU_MEMORY_V1:12345678-1234]] Archived memory"},
    {role: "user", text: "more old"},
    {role: "model", text: "another response"}
  ];
  const turns = [
    mockTurn("u1", "user"), mockTurn("m1", "model"),
    mockTurn("u2", "user"), mockTurn("m2", "model"),
    mockTurn("u3", "user"), mockTurn("m3", "model"),
    mockTurn("u4", "user"), mockTurn("m4", "model"),
    mockTurn("ut", "user"), mockTurn("mt", "model")
  ];
  turns[3].textContent = "[[SUJIU_MEMORY_V1:12345678-1234]] Archived memory";
  const x = context(turns);
  const plan = x.compactor.stage("[[SUJIU_MEMORY_V1:87654321-abc]] New memory", {
    older, totalRounds: 4, olderRounds: 3, recentRounds: 1
  });
  assert.deepEqual(Array.from(plan.pinnedOlderRounds), [1]);
  const selection = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(selection.targetIds), ["m1", "u1", "m3", "u3"]);
  assert.equal(selection.pinnedCount, 1);
  assert.equal(selection.deletedUserCount, 2);
});

test("cloud verification runs after the batch loop, not after every deleted message", () => {
  const loop = source.slice(source.indexOf("for (const id of preview.targetIds)"),
    source.indexOf("PromptPanel.status(\"本批次已在页面处理"));
  assert.doesNotMatch(loop, /fetchResolvedPromptThread|awaitCloudPersistence/);
  assert.match(source, /await awaitCloudPersistence\(plan, expectedUsers, expectedProtected, preview, saveBaseline\)/);
});

test("temporary tool prompt is selected but its marked model summary is protected", () => {
  const turn = (id, type, content) => {
    const t = mockTurn(id, type);
    t.textContent = content;
    return t;
  };
  const marker = "[[SUJIU_SCOPE_V2:12345678-1234]]";
  const pinned = "[[SUJIU_MEMORY_V1:12345678-1234]]";
  const turns = [
    turn("u1", "user", "old"), turn("m1", "model", "old model"),
    turn("u2", "user", "recent"), turn("m2", "model", "recent model"),
    turn("tool", "user", "Summarize " + marker),
    turn("thought", "thought", "hidden"),
    turn("memory", "model", pinned + " persistent summary")
  ];
  const x = context(turns);
  const plan = x.compactor.stage(pinned + " persistent summary", {
    older: [{role:"user",text:"old"}, {role:"model",text:"old model"}],
    totalRounds: 2, olderRounds: 1, recentRounds: 1
  }, { marker });
  assert.equal(plan.memoryMarker, pinned);
  const preview = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["m1", "u1", "thought", "tool"]);
  assert.ok(preview.protectedIds.includes("memory"));
  assert.ok(preview.protectedIds.includes("u2"));
  assert.equal(preview.deletedUserCount, 2);
});

test("an embedded thought without an independent menu stays protected", () => {
  const marker = "[[SUJIU_SCOPE_V2:12345678-1234]]";
  const pinned = "[[SUJIU_MEMORY_V1:12345678-1234]]";
  const turns = [
    mockTurn("u1", "user"), mockTurn("m1", "model"),
    mockTurn("u2", "user"), mockTurn("m2", "model"),
    mockTurn("tool", "user"), mockTurn("thought", "thought"), mockTurn("memory", "model")
  ];
  turns[4].textContent = marker;
  turns[6].textContent = pinned;
  turns[5].querySelector = () => null;
  const x = context(turns);
  const plan = x.compactor.stage(pinned, {
    older: [], totalRounds: 2, olderRounds: 1, recentRounds: 1
  }, { marker });
  const preview = x.compactor.preflightDom(plan);
  assert.equal(preview.skippedThoughts, 1);
  assert.ok(!preview.targetIds.includes("thought"));
  assert.ok(preview.protectedIds.includes("memory"));
});
