import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(new URL("../src/55-in-place.js", import.meta.url), "utf8");
const roundsSource = await readFile(new URL("../src/35-rounds.js", import.meta.url), "utf8");
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
function context(turns, overrides = {}) {
  let preview;
  let confirmationCalls = 0;
  const ctx = {
    document: { querySelectorAll: selector => selector === "ms-chat-turn" ? turns : [] },
    Branching: { getCurrentPromptId() { return "chat-test"; } },
    CompactionTask: {
      isPinnedSummary: text => /\[\[SUJIU_MEMORY_V1:/.test(text),
      memoryMarker: marker => marker.replace("SCOPE_V2", "MEMORY_V1"),
      getMemoryMarkers: text => [...String(text).matchAll(/\[\[SUJIU_MEMORY_V1:[a-z0-9-]{8,80}\]\]/gi)]
        .map(match => match[0])
    },
    log: { ui() {}, net() {} },
    SaveMonitor: { attempts: 0, latest: null },
    PromptPanel: { showCompactionReview(p) { preview = p; }, status() {} },
    state: { branchInProgress: false },
    userSettings: { keepRounds: 1 },
    fetchResolvedPromptThread: overrides.fetchResolvedPromptThread ||
      (async () => ({ entries: [] })),
    extractResolvedTurns: overrides.extractResolvedTurns || (() => []),
    extractResolvedEntryText: entry => entry[0] || "",
    isThoughtOnlyTurn: turn => turn?.role === "model" && turn?.entry?.[19] === 1,
    assertSupportedEntries() {},
    confirm() { confirmationCalls++; return false; },
    setTimeout,
    Promise
  };
  ctx.RoundSplitter = vm.runInNewContext(roundsSource + "\nRoundSplitter", {});
  const compactor = vm.runInNewContext(source + "\nInPlaceCompaction", ctx);
  return { compactor, state: ctx.state, get preview() { return preview; }, get confirmationCalls() { return confirmationCalls; } };
}
test("keeps the newest exchange and snapshot while selecting old thoughts for deletion", () => {
  const turns = [
    mockTurn("u1", "user"), mockTurn("t1", "thought"), mockTurn("m1", "text"),
    mockTurn("u2", "user"), mockTurn("m2", "model"),
    mockTurn("summary-prompt", "user"), mockTurn("summary-model", "model")
  ];
  const marker = "[[SUJIU_SCOPE_V2:12345678-0000]]";
  turns[5].textContent = marker;
  turns[6].textContent = "[[SUJIU_MEMORY_V1:12345678-0000]] Remember these events";
  const x = context(turns);
  const stage = x.compactor.stage("Remember these events", {
    totalRounds: 2, olderRounds: 1, recentRounds: 1
  }, { marker });
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
  turns[8].textContent = "[[SUJIU_SCOPE_V2:87654321-abcd]]";
  turns[9].textContent = "[[SUJIU_MEMORY_V1:87654321-abcd]] New memory";
  turns[3].textContent = "[[SUJIU_MEMORY_V1:12345678-1234]] Archived memory";
  const x = context(turns);
  const plan = x.compactor.stage("[[SUJIU_MEMORY_V1:87654321-abc]] New memory", {
    older, totalRounds: 3, olderRounds: 2, recentRounds: 1
  }, { marker: "[[SUJIU_SCOPE_V2:87654321-abcd]]" });
  assert.deepEqual(Array.from(plan.pinnedOlderRounds), []);
  const selection = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(selection.targetIds), ["m1", "u1", "m3", "u3"]);
  assert.equal(selection.protectedSummaryCount, 2);
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
  turns[7].textContent = "[[SUJIU_SCOPE_V2:87654321-abcd]]";
  turns[8].textContent = "[[SUJIU_MEMORY_V1:87654321-abcd]] New memory";
  turns[2].textContent = pinned + " old archive";
  const x = context(turns);
  const plan = x.compactor.stage("New model summary", {
    older, totalRounds:3, olderRounds:2, recentRounds:1
  }, { marker:"[[SUJIU_SCOPE_V2:87654321-abcd]]" });
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
    totalRounds:3, olderRounds:2, recentRounds:1
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


test("repeated compression excludes earlier archive requests when counting protected recent rounds", () => {
  const last = "[[SUJIU_SCOPE_V2:12345678-9999]]";
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("old-tool","user"),mockTurn("old-archive","model"),
    mockTurn("u2","user"),mockTurn("m2","model"),
    mockTurn("u3","user"),mockTurn("m3","model"),
    mockTurn("new-tool","user"),mockTurn("new-archive","model")
  ];
  turns[2].textContent="[[SUJIU_SCOPE_V2:12345678-aaaa]]";
  turns[3].textContent="[[SUJIU_MEMORY_V1:12345678-aaaa]] First memory";
  turns[8].textContent=last;
  turns[9].textContent="[[SUJIU_MEMORY_V1:12345678-9999]] Second memory";
  const x=context(turns);
  const plan=x.compactor.stage(turns[9].textContent,{
    totalRounds:3, olderRounds:1, recentRounds:2
  },{marker:last});
  const preview=x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds),["m1","u1"]);
  for(const id of ["old-tool","old-archive","u2","m2","u3","m3","new-tool","new-archive"])
    assert.ok(preview.protectedIds.includes(id),id+" must remain");
});

test("legacy XML snapshot and its English prompt survive later manual cleanup", () => {
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("legacy-prompt","user"), mockTurn("legacy-archive","model"),
    mockTurn("u2","user"), mockTurn("m2","model"),
    mockTurn("u3","user"), mockTurn("m3","model")
  ];
  turns[2].textContent = "Summarize the earlier role-play conversation. Return only: <state_snapshot>";
  turns[3].textContent = "<state_snapshot>Old events and relationships</state_snapshot>";
  const x = context(turns);
  const plan = x.compactor.planManualCleanup(1);
  assert.equal(plan.olderRounds, 2);
  const preview = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["m1","u1","m2","u2"]);
  for (const id of ["legacy-prompt","legacy-archive","u3","m3"]) {
    assert.ok(preview.protectedIds.includes(id), id + " must stay");
  }
  assert.equal(preview.protectedSummaryCount, 1);
});

test("orphaned XML summary and its predecessor stay protected during another compression", () => {
  const marker = "[[SUJIU_SCOPE_V2:12345678-1111]]";
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("old-memory","model"),
    mockTurn("u2","user"), mockTurn("m2","model"),
    mockTurn("latest-task","user"),mockTurn("latest-memory","model")
  ];
  turns[2].textContent = "<state_snapshot>Earliest events</state_snapshot>";
  turns[5].textContent = marker;
  turns[6].textContent = "[[SUJIU_MEMORY_V1:12345678-1111]] Updated story";
  const x=context(turns);
  const plan=x.compactor.stage(turns[6].textContent,{
    older:[], totalRounds:2, olderRounds:1, recentRounds:1
  },{marker});
  const preview=x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), []);
  for (const id of ["u1","m1","old-memory","u2","m2","latest-task","latest-memory"]) {
    assert.ok(preview.protectedIds.includes(id),id+" must survive");
  }
});

test("three compression generations are preserved by a cloud-verified inventory", () => {
  const turns = [
    mockTurn("old-u", "user"), mockTurn("old-m", "model"),
    mockTurn("task-a", "user"), mockTurn("memory-a", "model"),
    mockTurn("middle-u", "user"), mockTurn("middle-m", "model"),
    mockTurn("task-b", "user"), mockTurn("memory-b", "model"),
    mockTurn("recent-u", "user"), mockTurn("recent-m", "model"),
    mockTurn("task-c", "user"), mockTurn("memory-c", "model")
  ];
  for (const [i, suffix] of ["aaaa", "bbbb", "cccc"].entries()) {
    turns[[2, 6, 10][i]].textContent = "[[SUJIU_SCOPE_V2:12345678-" + suffix + "]]";
    turns[[3, 7, 11][i]].textContent = "[[SUJIU_MEMORY_V1:12345678-" + suffix + "]] Memory " + suffix;
  }
  const x = context(turns);
  const plan = x.compactor.planManualCleanup(1);
  const inventory = x.compactor.archiveInventory(turns,
    turn => turn.getAttribute("data-turn-type"), turn => turn.textContent);
  assert.equal(inventory.markers.size, 3);
  const selection = x.compactor.preflightDom(plan, inventory);
  for (const id of ["task-a", "memory-a", "task-b", "memory-b", "task-c", "memory-c"]) {
    assert.ok(selection.protectedIds.includes(id), id + " remains pinned");
  }
  assert.equal(selection.protectedSummaryCount, 3);
  assert.deepEqual(Array.from(selection.targetIds), ["old-m","old-u","middle-m","middle-u"]);
});

test("cloud and rendered archive counts can differ without blocking native cleanup", () => {
  const turns = [
    mockTurn("old-u", "user"), mockTurn("old-m", "model"),
    mockTurn("task-a", "user"), mockTurn("memory-a", "model"),
    mockTurn("recent-u", "user"), mockTurn("recent-m", "model")
  ];
  turns[2].textContent = "[[SUJIU_SCOPE_V2:12345678-aaaa]]";
  turns[3].textContent = "[[SUJIU_MEMORY_V1:12345678-aaaa]] Old summary";
  const x = context(turns);
  const expected = x.compactor.archiveInventory([
    ...turns,
    {role:"model", text:"[[SUJIU_MEMORY_V1:12345678-bbbb]] Hidden old summary"}
  ], turn => turn.role || turn.getAttribute("data-turn-type"),
    turn => turn.text || turn.textContent);
  assert.equal(expected.markers.size, 2);
  const selection = x.compactor.preflightDom(x.compactor.planManualCleanup(1), expected);
  assert.deepEqual(Array.from(selection.targetIds), ["old-m", "old-u"]);
  assert.ok(selection.protectedIds.includes("memory-a"));
  assert.ok(selection.protectedIds.includes("task-a"));
});

test("a marker quoted in a user message cannot fake a protected model result", () => {
  const x = context([]);
  const user = {role:"user",text:"[[SUJIU_MEMORY_V1:12345678-aaaa]]"};
  const a = x.compactor.archiveInventory([user], turn=>turn.role,turn=>turn.text);
  assert.equal(a.markers.size, 0);
  const model = {role:"model",text:"[[SUJIU_MEMORY_V1:12345678-aaaa]]"};
  const b = x.compactor.archiveInventory([user,model],turn=>turn.role,turn=>turn.text);
  assert.equal(b.markers.size, 1);
});

test("old unmarked archive records must also match the cloud inventory", () => {
  const x = context([]);
  const cloud = x.compactor.archiveInventory([
    {role:"user",text:"Summarize earlier conversation"},
    {role:"model",text:"【稳定设定】A\n【已发生的关键事件】B"}
  ], turn=>turn.role, turn=>turn.text);
  const page = x.compactor.archiveInventory([
    {role:"user",text:"Summarize earlier conversation"},
    {role:"model",text:"unmarked plain reply"}
  ], turn=>turn.role, turn=>turn.text);
  assert.throws(()=>x.compactor.verifyArchiveInventory(cloud,page),
    /保存的历史压缩摘要记录与此前读取结果不一致/);
});

test("autosave completion must be followed by cloud archive verification", () => {
  const code = source.slice(source.indexOf("async function commit("),source.indexOf("function cancel()"));
  assert.match(code,/savedInventory = await readSavedArchiveInventory\(\s*plan.promptId, Boolean\(plan.thoughtOnly\)\)/);
  assert.match(code,/preflightDom\(plan\)/);
  assert.match(code,/const afterInventory = await readSavedArchiveInventory\(\s*plan.promptId, Boolean\(plan.thoughtOnly\)\)/);
  assert.match(code,/verifyArchiveInventory\(savedInventory, afterInventory\)/);
});

test("standalone cleanup removes every independent thought including recent rounds and archives", () => {
  const turns = [
    mockTurn("old-user","user"),mockTurn("old-thought","thought"),mockTurn("old-reply","model"),
    mockTurn("summary-task","user"),mockTurn("summary-thought","thought"),mockTurn("summary-reply","model"),
    mockTurn("recent-user1","user"),mockTurn("recent-thought1","thought"),mockTurn("recent-reply1","model"),
    mockTurn("recent-user2","user"),mockTurn("recent-thought2","thought"),mockTurn("recent-reply2","model")
  ];
  turns[3].textContent = "Summarize [[SUJIU_SCOPE_V2:12345678-aaaa]]";
  turns[5].textContent = "[[SUJIU_MEMORY_V1:12345678-aaaa]] First archive";
  const x = context(turns);
  const plan = x.compactor.planThoughtCleanup(2);
  assert.equal(plan.thoughtOnly, true);
  const selection = x.compactor.preflightDom(plan);
  assert.equal(selection.thoughtOnly, true);
  assert.deepEqual(Array.from(selection.targetIds), ["old-thought","summary-thought","recent-thought1","recent-thought2"]);
  assert.equal(selection.deletedUserCount, 0);
  assert.equal(selection.deletedOldRounds, 0);
  assert.equal(selection.protectedSummaryCount, 1);
  for (const id of ["old-user","old-reply","summary-task","summary-reply",
    "recent-user1","recent-reply1",
    "recent-user2","recent-reply2"]) {
    assert.ok(selection.protectedIds.includes(id), id + " must survive");
  }
});

test("standalone cleanup skips old thoughts without independent native delete menu", () => {
  const turns = [
    mockTurn("old-user","user"),mockTurn("old-thought","thought"),mockTurn("old-reply","model"),
    mockTurn("new-user","user"),mockTurn("new-reply","model")
  ];
  turns[1].querySelector = () => null;
  const x = context(turns);
  const selection = x.compactor.preflightDom(x.compactor.planThoughtCleanup(1));
  assert.deepEqual(Array.from(selection.targetIds), []);
  assert.equal(selection.skippedThoughts, 1);
  assert.ok(selection.protectedIds.includes("old-reply"));
});

test("standalone cleanup rejects a changed or virtualized live DOM", () => {
  const turns = [
    mockTurn("u0","user"),mockTurn("t0","thought"),mockTurn("m0","model"),
    mockTurn("u1","user"),mockTurn("m1","model")
  ];
  const x = context(turns);
  const plan = x.compactor.planThoughtCleanup(1);
  turns.splice(1, 1);
  assert.throws(() => x.compactor.preflightDom(plan), /网页聊天节点在准备期间发生变化/);
});

test("thought-only cleanup ignores a mismatched cloud archive list", () => {
  const turns = [
    mockTurn("task","user"), mockTurn("archive-thought","thought"),
    mockTurn("archive","model"),mockTurn("recent-user","user"),mockTurn("recent-model","model")
  ];
  turns[0].textContent = "[[SUJIU_SCOPE_V2:12345678-aaaa]] Summarize";
  turns[2].textContent = "[[SUJIU_MEMORY_V1:12345678-aaaa]] Old summary";
  const x = context(turns);
  const cloud = x.compactor.archiveInventory([
    ...turns,
    {role:"model",text:"[[SUJIU_MEMORY_V1:12345678-bbbb]] Missing"}
  ], turn => turn.role || turn.getAttribute("data-turn-type"), turn => turn.text || turn.textContent);
  const selection = x.compactor.preflightDom(x.compactor.planThoughtCleanup(1), cloud);
  assert.deepEqual(Array.from(selection.targetIds), ["archive-thought"]);
  assert.ok(selection.protectedIds.includes("archive"));
  assert.ok(selection.protectedIds.includes("task"));
});

test("post-delete normal user/model replies must remain exactly intact, including unmarked history", () => {
  const x = context([]);
  const before = {
    protectedMessages: [
      ["user","Once upon a time"],["model","Ordinary response"],
      ["user","Summarize"],["model","Old unmarked summary"]
    ]
  };
  const afterThought = {
    protectedMessages: before.protectedMessages.map(item => [...item])
  };
  assert.doesNotThrow(() => x.compactor.verifyProtectedMessages(before,afterThought));
  const afterLostReply = {
    protectedMessages: [
      ["user","Once upon a time"],["user","Summarize"],["model","Old unmarked summary"]
    ]
  };
  assert.throws(() => x.compactor.verifyProtectedMessages(before,afterLostReply),
    /普通对话或摘要记录与此前读取结果不一致/);
});

test("thought-only commit checks preserved cloud replies after native autosave", () => {
  const code = source.slice(source.indexOf("async function commit("),source.indexOf("function cancel()"));
  assert.match(code,/savedInventory = await readSavedArchiveInventory\(\s*plan.promptId, Boolean\(plan.thoughtOnly\)\)/);
  assert.match(code,/const afterInventory = await readSavedArchiveInventory\(\s*plan.promptId, Boolean\(plan.thoughtOnly\)\)/);
  assert.match(code,/if \(plan.thoughtOnly\) \{\s*verifyProtectedMessages\(savedInventory, afterInventory\)/);
  assert.match(code,/const preview = preflightDom\(plan\)/);
});

test("native delete rechecks a thought node before clicking the destructive menu", () => {
  const code = source.slice(source.indexOf("async function deleteOne("),source.indexOf("async function commit("));
  assert.match(code,/if \(thoughtOnly && normalizeType\(turn\) !== "thought"\)/);
  assert.match(source,/await deleteOne\(id, Boolean\(plan.thoughtOnly\)\)/);
});

test("thinking cleanup ignores unknown search/tool nodes and failed generations", () => {
  const turns = [
    mockTurn("u","user"), mockTurn("think-before","thought"),
    mockTurn("google-search","tool"), mockTurn("search-error","error"),
    mockTurn("model-result","model"), mockTurn("incomplete-user","user"),
    mockTurn("think-after","thought")
  ];
  const x = context(turns);
  const plan = x.compactor.planThoughtCleanup();
  const preview = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["think-before","think-after"]);
  for (const id of ["u","google-search","search-error","model-result","incomplete-user"])
    assert.ok(preview.protectedIds.includes(id), id + " must be preserved");
});

test("all thought cleanup has no recent-turn preservation window", () => {
  const turns=[mockTurn("u1","user"),mockTurn("t1","thought"),mockTurn("m1","model"),
               mockTurn("u2","user"),mockTurn("t2","thought"),mockTurn("m2","model")];
  const x=context(turns);
  const preview=x.compactor.preflightDom(x.compactor.planThoughtCleanup());
  assert.deepEqual(Array.from(preview.targetIds),["t1","t2"]);
});

test("cloud preflight and DOM failures are covered by unconditional lock release", () => {
  const body=source.slice(source.indexOf("async function commit("), source.indexOf("function cancel()"));
  const acquire=body.indexOf("deleting = true;");
  const cloud=body.indexOf("savedInventory = await readSavedArchiveInventory(");
  const release=body.lastIndexOf("deleting = false;");
  assert.ok(acquire >= 0 && acquire < cloud, "lock acquired before cloud inspection");
  assert.ok(release > cloud, "the lock is released on preflight failure");
  assert.match(body,/finally \{[\s\S]*?deleting = false;[\s\S]*?state.branchInProgress = false;[\s\S]*?\}/);
});

test("thought-only cloud inventory accepts opaque search tool entries without strict two-role assertion", () => {
  const code=source.slice(source.indexOf("async function readSavedArchiveInventory"),
    source.indexOf("function verifyProtectedMessages"));
  assert.match(code,/if \(!allowOpaque\) assertSupportedEntries\(capture.entries, turns\)/);
  assert.match(code,/entry.length >= 9/);
  assert.match(code,/text \|\| JSON.stringify\(entry\)/);
});

test("legacy dialogue cleanup still requires strict validation of cloud entries", () => {
  const body=source.slice(source.indexOf("async function commit("),source.indexOf("function cancel()"));
  assert.match(body,/plan.promptId, Boolean\(plan.thoughtOnly\)/);
  assert.match(source,/if \(!allowOpaque\) assertSupportedEntries\(capture.entries, turns\)/);
});

test("cloud preflight failures do not block retries or leave the lock held", async () => {
  const x = context([mockTurn("u","user"),mockTurn("t","thought"),mockTurn("m","model")]);
  for (let attempt = 0; attempt < 2; attempt++) {
    await x.compactor.cleanupThoughts();
    assert.equal(x.state.branchInProgress, false);
  }
  assert.equal(x.confirmationCalls, 2);
});

test("cloud tool records and incomplete page turns do not block thought cleanup preflight", async () => {
  const entry = (role, content) => {
    const result = new Array(9).fill(null);
    result[0] = content;
    result[8] = role;
    return result;
  };
  const cloud = [
    entry("user", "Hello"),
    entry("tool", "web_search result"),
    entry("model", "Search failed, but conversation can continue"),
    entry("user", "Unfinished action due to error")
  ];
  const turns = [
    mockTurn("u","user"),mockTurn("search","tool"),
    mockTurn("reason","thought"),mockTurn("answer","model"),
    mockTurn("incomplete","user")
  ];
  const x = context(turns, {
    fetchResolvedPromptThread: async () => ({entries: cloud}),
    extractResolvedTurns: entries => entries.map(e => ({
      role:e[8], text:e[0], entry:e
    }))
  });
  await x.compactor.cleanupThoughts();
  assert.equal(x.confirmationCalls, 1,
    "the confirmation dialog should be reached despite opaque tool/error turns");
  assert.equal(x.state.branchInProgress, false);
});

test("cloud-to-DOM mismatch does not prevent the confirmation dialog", async () => {
  const cloudTurn = (role, text) => {
    const entry = new Array(9).fill(null);
    entry[0] = text;
    entry[8] = role;
    return entry;
  };
  const turns = [
    mockTurn("u","user"),mockTurn("thinking","thought"),mockTurn("m","model")
  ];
  const x = context(turns, {
    fetchResolvedPromptThread: async () => ({
      entries: [
        cloudTurn("user", "Hello"),
        cloudTurn("model", "[[SUJIU_MEMORY_V1:12345678-abcd]] Archive missing from DOM")
      ]
    }),
    extractResolvedTurns: entries => entries.map(entry => ({
      role: entry[8], text: entry[0], entry
    }))
  });
  await x.compactor.cleanupThoughts();
  assert.equal(x.confirmationCalls, 1);
  assert.equal(x.state.branchInProgress, false);
});

test("native deletion still protects explicit summary and scope markers", () => {
  const code=source.slice(source.indexOf("async function deleteOne("),
    source.indexOf("async function commit("));
  assert.match(code,/SUJIU_\(\?:MEMORY_V1\|SCOPE_V2\)/);
  assert.match(code,/目标带有素酒永久记忆标识/);
});

test("cloud-vs-DOM archive inventory mismatch is never a deletion gate", () => {
  const code=source.slice(source.indexOf("function preflightDom("),
    source.indexOf("async function findNativeDeleteItem("));
  assert.doesNotMatch(code,/verifyArchiveInventory\(/);
  const commit=source.slice(source.indexOf("async function commit("),
    source.indexOf("function cancel()"));
  assert.match(commit,/let savedInventory = null/);
  assert.match(commit,/const preview = preflightDom\(plan\)/);
  assert.match(commit,/let verificationNotice = ""/);
});

