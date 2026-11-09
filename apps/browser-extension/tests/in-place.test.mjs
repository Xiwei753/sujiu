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
    userSettings: { keepRounds: 1 },
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
  }), /历史轮次已改变/);
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

test("native deletion autosaves without a separate Save click or cloud polling", () => {
  const loopStart = source.indexOf("for (const id of preview.targetIds)");
  const wait = source.indexOf("SaveMonitor.waitForNativeSave(", loopStart);
  assert.ok(wait > loopStart);
  const loop = source.slice(loopStart, wait);
  assert.doesNotMatch(loop, /fetchResolvedPromptThread|awaitCloudPersistence|NativeSave\.triggerOnce/);
  assert.equal((source.match(/SaveMonitor\.waitForNativeSave\(/g) || []).length, 1);
  assert.match(source, /lastDeletionStartedAt = Date\.now\(\)/);
  assert.match(source, /waitForNativeSave\(saveBaseline, lastDeletionStartedAt\)/);
});

test("temporary tool prompt is retained when deleting it could cascade to the marked summary", () => {
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
  assert.deepEqual(Array.from(preview.targetIds), ["m1", "u1", "thought"]);
  assert.ok(preview.protectedIds.includes("tool"), "never risk deleting a paired model reply via its user prompt");
  assert.ok(preview.protectedIds.includes("memory"));
  assert.ok(preview.protectedIds.includes("u2"));
  assert.equal(preview.deletedUserCount, 1);
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

test("standalone pinned memories survive future compression without protecting unrelated old rounds", () => {
  const pinned = "[[SUJIU_MEMORY_V1:12345678-abcd]]";
  const older = [
    {role:"user", text:"first"}, {role:"model", text:"first reply"},
    {role:"model", text:pinned + " old archive"},
    {role:"user", text:"second"}, {role:"model", text:"second reply"}
  ];
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("memory-old","model"),
    mockTurn("u2","user"), mockTurn("m2","model"),
    mockTurn("u3","user"), mockTurn("m3","model"),
    mockTurn("tool","user"), mockTurn("new-memory","model")
  ];
  turns[2].textContent = pinned + " old archive";
  const x = context(turns);
  const plan = x.compactor.stage("New model summary", {
    older, totalRounds:3, olderRounds:2, recentRounds:1
  });
  assert.deepEqual(Array.from(plan.pinnedOlderRounds), []);
  assert.deepEqual(Array.from(plan.pinnedMemoryTexts), [pinned + " old archive"]);
  const selection = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(selection.targetIds), ["m2", "u2"]);
  assert.ok(selection.protectedIds.includes("u1"), "neighboring user turn stays protected against cascading deletion");
  assert.ok(selection.protectedIds.includes("m1"));
  assert.ok(selection.protectedIds.includes("memory-old"));
  assert.ok(!selection.targetIds.includes("memory-old"));
});


test("manual cleanup can target old complete rounds without any generated summary or task marker", () => {
  const turns = [
    mockTurn("u1","user"), mockTurn("t1","thought"), mockTurn("m1","model"),
    mockTurn("u2","user"), mockTurn("m2","model"),
    mockTurn("u3","user"), mockTurn("m3","model"),
  ];
  const x = context(turns);
  const plan = x.compactor.planManualCleanup(1);
  assert.equal(plan.manual, true);
  assert.equal(plan.marker, null);
  assert.equal(plan.memoryMarker, null);
  assert.equal(plan.totalRounds, 3);
  assert.equal(plan.olderRounds, 2);
  const preview = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["m1","t1","u1","m2","u2"]);
  assert.ok(preview.protectedIds.includes("u3"));
  assert.ok(preview.protectedIds.includes("m3"));
});

test("manual cleanup always pins older marked compression memories", () => {
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("task","user"), mockTurn("archive","model"),
    mockTurn("u3","user"), mockTurn("m3","model"),
  ];
  turns[3].textContent = "[[SUJIU_MEMORY_V1:12345678-abcd]] Historical archive";
  const x = context(turns);
  const plan = x.compactor.planManualCleanup(1);
  assert.deepEqual(Array.from(plan.pinnedOlderRounds), [1]);
  const preview = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["m1","u1"]);
  assert.ok(preview.protectedIds.includes("archive"));
  assert.equal(preview.pinnedCount, 1);
});

test("manual plan becomes invalid if chat DOM changes before deletion starts", () => {
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("u2","user"), mockTurn("m2","model"),
  ];
  const x = context(turns);
  const plan = x.compactor.planManualCleanup(1);
  turns[0].id = "unexpected-id";
  assert.throws(() => x.compactor.preflightDom(plan), /节点在准备期间发生变化/);
});

test("manual cleanup path skips generated-summary verification, but still requires confirmation", () => {
  const code = source.slice(source.indexOf("async function commit("), source.indexOf("function cancel()"));
  assert.match(code, /if \(!plan\.manual\) await verifyPersistedSummary\(plan\)/);
  assert.match(code, /if \(!confirm\(prompt\)\) return/);
  assert.match(code, /const preview = preflightDom\(plan\)/);
  assert.match(source, /async function manualCleanup\(\)/);
});


test("marked summary/tool exchange does not consume one of the protected recent chat rounds", () => {
  const turns = [
    mockTurn("u1", "user"), mockTurn("m1", "model"),
    mockTurn("task", "user"), mockTurn("archive", "model"),
    mockTurn("u2", "user"), mockTurn("m2", "model"),
    mockTurn("u3", "user"), mockTurn("m3", "model")
  ];
  turns[2].textContent = "summary command [[SUJIU_SCOPE_V2:12345678-abcd]]";
  turns[3].textContent = "[[SUJIU_MEMORY_V1:12345678-abcd]] Archived memory";
  const x = context(turns);
  const plan = x.compactor.planManualCleanup(2);
  assert.equal(plan.totalRounds, 4);
  assert.equal(plan.olderRounds, 1);
  assert.equal(plan.recentRounds, 2);
  assert.deepEqual(Array.from(plan.olderGroupIndices), [0]);
  const selection = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(selection.targetIds), ["m1","u1"]);
  assert.ok(selection.protectedIds.includes("task"));
  assert.ok(selection.protectedIds.includes("archive"));
  assert.ok(selection.protectedIds.includes("u2"));
  assert.ok(selection.protectedIds.includes("u3"));
});


test("manual cleanup protects an older tool summary even without a memory marker", () => {
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("old-task","user"), mockTurn("old-thought","thought"), mockTurn("old-result","model"),
    mockTurn("u2","user"), mockTurn("m2","model"),
    mockTurn("u3","user"), mockTurn("m3","model")
  ];
  turns[2].textContent = "Summarize history [[SUJIU_SCOPE_V2:12345678-abcd]]";
  turns[4].textContent = "Old summary without a marker";
  const x = context(turns);
  const plan = x.compactor.planManualCleanup(1);
  assert.deepEqual(Array.from(plan.olderGroupIndices), [0, 2]);
  const preview = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["m1","u1","m2","u2","old-thought"]);
  assert.ok(preview.protectedIds.includes("old-result"), "old summary reply is retained");
  assert.ok(preview.protectedIds.includes("old-task"), "old summary command is kept to avoid cascading delete");
  assert.equal(preview.protectedSummaryCount, 1);
});

test("unmarked structured summary is retained even if its user command looks ordinary", () => {
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("old-ask","user"), mockTurn("old-summary","model"),
    mockTurn("u2","user"), mockTurn("m2","model")
  ];
  turns[2].textContent = "帮我概括一下吧";
  turns[3].textContent = "【历史范围与时间锚点】\n前情\n【关键事件时间线】\n初遇和结盟";
  const x = context(turns);
  const plan = x.compactor.planManualCleanup(1);
  const preview = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["m1","u1"]);
  assert.ok(preview.protectedIds.includes("old-summary"));
  assert.ok(preview.protectedIds.includes("old-ask"));
  assert.equal(plan.olderRounds, 1);
});

test("old marked compression keeps its model reply and prompt; only independent thinking may be removed", () => {
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("task","user"), mockTurn("think","thought"), mockTurn("memory","model"),
    mockTurn("u2","user"), mockTurn("m2","model")
  ];
  turns[2].textContent = "[[SUJIU_SCOPE_V2:12345678-1111]] Summarize";
  turns[4].textContent = "[[SUJIU_MEMORY_V1:12345678-1111]] Narrative memory";
  const x = context(turns);
  const plan = x.compactor.planManualCleanup(1);
  const preview = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["m1","u1","think"]);
  for (const id of ["task","memory"]) assert.ok(preview.protectedIds.includes(id));
  assert.equal(preview.protectedSummaryCount, 1);
});

test("a new compression keeps old unmarked summaries and the newest result", () => {
  const marker = "[[SUJIU_SCOPE_V2:12345678-2222]]";
  const latestMemory = "[[SUJIU_MEMORY_V1:12345678-2222]]";
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("old-task","user"), mockTurn("old-summary","model"),
    mockTurn("u2","user"), mockTurn("m2","model"),
    mockTurn("recent","user"), mockTurn("recent-model","model"),
    mockTurn("new-task","user"), mockTurn("new-thought","thought"), mockTurn("new-summary","model")
  ];
  turns[2].textContent = "历史整理 [[SUJIU_SCOPE_V2:12345678-1111]]";
  turns[3].textContent = "Old content without memory marker";
  turns[8].textContent = marker;
  turns[10].textContent = latestMemory + " Latest scene";
  const x = context(turns);
  const plan = x.compactor.stage(latestMemory + " Latest scene", {
    older: [{role:"user",text:"u1"}, {role:"model",text:"m1"},
            {role:"user",text:turns[2].textContent}, {role:"model",text:turns[3].textContent},
            {role:"user",text:"u2"}, {role:"model",text:"m2"}],
    totalRounds:4, olderRounds:3, recentRounds:1
  }, { marker });
  const preview = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["m1","u1","m2","u2","new-thought"]);
  assert.ok(preview.protectedIds.includes("old-summary"));
  assert.ok(preview.protectedIds.includes("new-summary"));
  assert.ok(preview.protectedIds.includes("old-task"));
  assert.ok(preview.protectedIds.includes("new-task"));
});

test("a summary lacking a standalone thought delete menu keeps the thought", () => {
  const marker = "[[SUJIU_SCOPE_V2:12345678-1111]]";
  const turns = [
    mockTurn("u1","user"),mockTurn("m1","model"),
    mockTurn("task","user"),mockTurn("think","thought"),mockTurn("memory","model"),
    mockTurn("u2","user"),mockTurn("m2","model")
  ];
  turns[2].textContent = marker;
  turns[4].textContent = "[[SUJIU_MEMORY_V1:12345678-1111]] content";
  turns[3].querySelector = () => null;
  const x = context(turns);
  const plan = x.compactor.planManualCleanup(1);
  const preview = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["m1","u1"]);
  assert.ok(preview.protectedIds.includes("think"), "unremovable thought stays protected");
  assert.equal(preview.skippedThoughts, 1);
});


test("manual cleanup can delete only independent archive thoughts without deleting any ordinary history", () => {
  const turns = [
    mockTurn("task","user"), mockTurn("think","thought"), mockTurn("memory","model"),
    mockTurn("recent","user"), mockTurn("reply","model")
  ];
  turns[0].textContent = "Summarize [[SUJIU_SCOPE_V2:12345678-aaaa]]";
  turns[2].textContent = "[[SUJIU_MEMORY_V1:12345678-aaaa]] Summary kept";
  const x = context(turns);
  const plan = x.compactor.planManualCleanup(1);
  assert.equal(plan.olderRounds, 0);
  const preview = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["think"]);
  assert.ok(preview.protectedIds.includes("task"));
  assert.ok(preview.protectedIds.includes("memory"));
  assert.ok(preview.protectedIds.includes("recent"));
  assert.ok(preview.protectedIds.includes("reply"));
});
