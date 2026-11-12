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
  const statuses = [];
  const ctx = {
    document: overrides.document || {
      querySelectorAll: selector => selector === "ms-chat-turn" ? turns : [],
      getElementById: id => turns.find(turn => turn.id === id) || null
    },
    Branching: { getCurrentPromptId() { return "chat-test"; } },
    CompactionTask: {
      isPinnedSummary: text => /\[\[SUJIU_MEMORY_V1:/.test(text),
      isProtectedMessage: text => /\[\[(?:SUJIU_PROTECT_V1|SUJIU_MEMORY_V1:[a-z0-9-]{8,80})\]\]/i.test(String(text)),
      memoryMarker: marker => marker.replace("SCOPE_V2", "MEMORY_V1"),
      getMemoryMarkers: text => [...String(text).matchAll(/\[\[SUJIU_MEMORY_V1:[a-z0-9-]{8,80}\]\]/gi)]
        .map(match => match[0])
    },
    log: { ui() {}, net() {} },
    SaveMonitor: overrides.SaveMonitor || { attempts: 0, latest: null },
    PromptPanel: {
      showCompactionReview(p) { preview = p; },
      status(message) { statuses.push(String(message)); }
    },
    state: { branchInProgress: false },
    userSettings: { keepRounds: 1 },
    fetchResolvedPromptThread: overrides.fetchResolvedPromptThread ||
      (async () => ({ entries: [] })),
    extractResolvedTurns: overrides.extractResolvedTurns || (() => []),
    extractResolvedEntryText: entry => entry[0] || "",
    isThoughtOnlyTurn: turn => turn?.role === "model" && turn?.entry?.[19] === 1,
    assertSupportedEntries() {},
    confirm() {
      confirmationCalls++;
      overrides.onConfirm?.();
      return Boolean(overrides.confirmResult);
    },
    PointerEvent: class { constructor() {} },
    MouseEvent: class { constructor() {} },
    setTimeout,
    Promise
  };
  ctx.RoundSplitter = vm.runInNewContext(roundsSource + "\nRoundSplitter", {});
  const compactor = vm.runInNewContext(source + "\nInPlaceCompaction", ctx);
  return { compactor, state: ctx.state,
    get preview() { return preview; },
    get statuses() { return statuses; },
    get confirmationCalls() { return confirmationCalls; }
  };
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
  assert.deepEqual(Array.from(selection.targetIds), ["m1", "t1", "u1", "summary-prompt"]);
  assert.equal(selection.groups, 3)
  assert.ok(selection.protectedIds.includes("summary-model"));;
  assert.equal(selection.items, 4);
  assert.equal(x.confirmationCalls, 0);
});
test("never deletes when older messages are not fully rendered", () => {
  const x = context([mockTurn("u1","user"),mockTurn("m1","model")]);
  assert.throws(() => x.compactor.preflightDom({
    totalRounds: 2, olderRounds: 1, recentRounds: 1
  }), /历史轮次已改变/);
});
test("unknown and incomplete messages can be preserved without blocking cleanup", () => {
  const x = context([]);
  assert.doesNotThrow(() => x.compactor.collectGroups([mockTurn("u", "user"),mockTurn("x", "other")]));
  assert.doesNotThrow(() => x.compactor.collectGroups([mockTurn("u", "user")]));
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
  assert.deepEqual(Array.from(selection.targetIds), ["m1", "u1", "m3", "u3", "ut"]);
  assert.equal(selection.protectedSummaryCount, 2);
  assert.equal(selection.deletedUserCount, 3);
});

test("native deletion autosaves without a separate Save click or cloud polling", () => {
  const loopStart = source.indexOf("for (const id of preview.targetIds)");
  const wait = source.indexOf("SaveMonitor.waitForNativeSave(", loopStart);
  assert.ok(wait > loopStart);
  const loop = source.slice(loopStart, wait);
  assert.doesNotMatch(loop, /fetchResolvedPromptThread|awaitCloudPersistence|NativeSave\.triggerOnce/);
  assert.equal((source.match(/SaveMonitor\.waitForNativeSave\(/g) || []).length, 1);
  assert.match(source, /lastDeletionStartedAt = attemptStartedAt/);
  assert.match(source, /waitForNativeSave\(saveBaseline, lastDeletionStartedAt\)/);
});

test("temporary tool prompt and thinking are deleted while the marked model summary stays pinned", () => {
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
  assert.ok(!preview.protectedIds.includes("tool"), "temporary task prompt is disposable");
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
  assert.deepEqual(Array.from(selection.targetIds), ["m1", "u1", "m2", "u2", "tool"]);
  assert.ok(!selection.protectedIds.includes("u1"), "unmarked preceding user is not protected");
  assert.ok(!selection.protectedIds.includes("m1"));
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
  assert.deepEqual(Array.from(selection.targetIds), ["m1","u1","task"]);
  assert.ok(!selection.protectedIds.includes("task"));
  assert.ok(selection.protectedIds.includes("archive"));
  assert.ok(selection.protectedIds.includes("u2"));
  assert.ok(selection.protectedIds.includes("u3"));
});


test("manual cleanup protects only explicitly marked old model summaries", () => {
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("old-task","user"), mockTurn("old-thought","thought"), mockTurn("old-result","model"),
    mockTurn("u2","user"), mockTurn("m2","model"),
    mockTurn("u3","user"), mockTurn("m3","model")
  ];
  turns[2].textContent = "Summarize history [[SUJIU_SCOPE_V2:12345678-abcd]]";
  turns[4].textContent = "[[SUJIU_MEMORY_V1:12345678-abcd]] Old summary";
  const x = context(turns);
  const plan = x.compactor.planManualCleanup(1);
  assert.deepEqual(Array.from(plan.olderGroupIndices), [0, 2]);
  const preview = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["m1","u1","m2","u2","old-thought","old-task"]);
  assert.ok(preview.protectedIds.includes("old-result"), "old summary reply is retained");
  assert.ok(!preview.protectedIds.includes("old-task"), "the temporary auto-generated request is disposable");
  assert.equal(preview.protectedSummaryCount, 1);
});

test("user-supplied protection marker pins their own message and its associated reply", () => {
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("old-ask","user"), mockTurn("old-summary","model"),
    mockTurn("u2","user"), mockTurn("m2","model")
  ];
  turns[2].textContent = "[[SUJIU_PROTECT_V1]] 帮我概括一下吧";
  turns[3].textContent = "【历史范围与时间锚点】\n前情\n【关键事件时间线】\n初遇和结盟";
  const x = context(turns);
  const plan = x.compactor.planManualCleanup(1);
  const preview = x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["m1","u1"]);
  assert.ok(preview.protectedIds.includes("old-summary"));
  assert.ok(preview.protectedIds.includes("old-ask"));
  assert.equal(plan.olderRounds, 1);
});

test("old marked compression keeps only model reply, removing auto-prompt and independent thinking", () => {
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
  assert.deepEqual(Array.from(preview.targetIds), ["m1","u1","think","task"]);
  assert.ok(preview.protectedIds.includes("memory"));
  assert.ok(!preview.protectedIds.includes("task"));
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
  assert.deepEqual(Array.from(preview.targetIds), ["m1","u1","m2","u2","new-thought","old-task","new-task"]);
  assert.ok(preview.protectedIds.includes("old-summary"));
  assert.ok(preview.protectedIds.includes("new-summary"));
  assert.ok(!preview.protectedIds.includes("old-task"));
  assert.ok(!preview.protectedIds.includes("new-task"));
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
  assert.deepEqual(Array.from(preview.targetIds), ["m1","u1","task"]);
  assert.ok(preview.protectedIds.includes("think"), "unremovable thought stays protected");
  assert.equal(preview.skippedThoughts, 1);
});


test("manual cleanup also deletes temporary task and keeps all ordinary messages", () => {
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
  assert.deepEqual(Array.from(preview.targetIds), ["think","task"]);
  assert.ok(!preview.protectedIds.includes("task"));
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
  assert.deepEqual(Array.from(preview.targetIds),["m1","u1","old-tool","new-tool"]);
  for(const id of ["old-archive","u2","m2","u3","m3","new-archive"])
    assert.ok(preview.protectedIds.includes(id),id+" must remain");
});

test("legacy XML wording has no effect, explicit memory marker protects archive", () => {
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("legacy-prompt","user"), mockTurn("legacy-archive","model"),
    mockTurn("u2","user"), mockTurn("m2","model"),
    mockTurn("u3","user"), mockTurn("m3","model")
  ];
  turns[2].textContent = "Summarize the earlier role-play conversation. Return only: <state_snapshot>";
  turns[3].textContent = "[[SUJIU_MEMORY_V1:12345678-cafe]] <state_snapshot>Old events and relationships</state_snapshot>";
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

test("marked orphan memory remains protected but unmarked predecessor follows normal cleanup", () => {
  const marker = "[[SUJIU_SCOPE_V2:12345678-1111]]";
  const turns = [
    mockTurn("u1","user"), mockTurn("m1","model"),
    mockTurn("old-memory","model"),
    mockTurn("u2","user"), mockTurn("m2","model"),
    mockTurn("latest-task","user"),mockTurn("latest-memory","model")
  ];
  turns[2].textContent = "[[SUJIU_MEMORY_V1:12345678-ffff]] <state_snapshot>Earliest events</state_snapshot>";
  turns[5].textContent = marker;
  turns[6].textContent = "[[SUJIU_MEMORY_V1:12345678-1111]] Updated story";
  const x=context(turns);
  const plan=x.compactor.stage(turns[6].textContent,{
    older:[], totalRounds:2, olderRounds:1, recentRounds:1
  },{marker});
  const preview=x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(preview.targetIds), ["m1","u1","latest-task"]);
  for (const id of ["old-memory","u2","m2","latest-memory"]) {
    assert.ok(preview.protectedIds.includes(id),id+" must survive");
  }
});

test("three archived model summaries remain pinned while all their temporary prompts are selected", () => {
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
  for (const id of ["memory-a", "memory-b", "memory-c"]) {
    assert.ok(selection.protectedIds.includes(id), id + " remains pinned");
  }
  assert.equal(selection.protectedSummaryCount, 3);
  assert.deepEqual(Array.from(selection.targetIds), ["old-m","old-u","middle-m","middle-u","task-a","task-b","task-c"]);
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
  assert.deepEqual(Array.from(selection.targetIds), ["old-m", "old-u", "task-a"]);
  assert.ok(selection.protectedIds.includes("memory-a"));
  assert.ok(!selection.protectedIds.includes("task-a"));
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

test("unmarked text differences are not treated as loss of protected messages", () => {
  const x = context([]);
  const cloud = x.compactor.archiveInventory([
    {role:"user",text:"Summarize earlier conversation"},
    {role:"model",text:"【稳定设定】A\n【已发生的关键事件】B"}
  ], turn=>turn.role, turn=>turn.text);
  const page = x.compactor.archiveInventory([
    {role:"user",text:"Summarize earlier conversation"},
    {role:"model",text:"unmarked plain reply"}
  ], turn=>turn.role, turn=>turn.text);
  assert.doesNotThrow(()=>x.compactor.verifyArchiveInventory(cloud,page));
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
  assert.deepEqual(Array.from(selection.targetIds), ["old-thought","summary-thought","recent-thought1","recent-thought2","summary-task"]);
  assert.equal(selection.deletedUserCount, 1);
  assert.equal(selection.deletedOldRounds, 0);
  assert.equal(selection.protectedSummaryCount, 1);
  for (const id of ["old-user","old-reply","summary-reply",
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
  assert.deepEqual(Array.from(selection.targetIds), ["archive-thought","task"]);
  assert.ok(selection.protectedIds.includes("archive"));
  assert.ok(!selection.protectedIds.includes("task"));
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
  assert.match(code,/if \(thoughtOnly && type !== "thought" && !isTask\)/);
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

test("native deletion checks explicit marker for either user or model but never thinking", () => {
  const code=source.slice(source.indexOf("async function deleteOne("),
    source.indexOf("async function commit("));
  assert.match(code,/type !== "thought" && isProtectedTurn\(turn\)/);
  assert.match(code,/消息带有素酒保护标识符/);
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

test("auto-generated task with unmarked model archive is still disposable, not a user-written recap", () => {
  const turns = [
    mockTurn("prompt", "user"), mockTurn("thinking", "thought"),
    mockTurn("model", "model")
  ];
  turns[0].textContent = "Please summarize [[SUJIU_SCOPE_V2:12345678-aaaa]]";
  turns[2].textContent = "【稳定设定】A\n【已发生的关键事件】B";
  const x = context(turns);
  const selection = x.compactor.preflightDom(x.compactor.planThoughtCleanup());
  assert.deepEqual(Array.from(selection.targetIds), ["thinking", "prompt"]);
  assert.ok(selection.protectedIds.includes("model"));
});

test("user-authored summary requests without the plugin task marker remain protected", () => {
  const turns = [
    mockTurn("manual-prompt", "user"), mockTurn("thought", "thought"),
    mockTurn("result", "model")
  ];
  turns[0].textContent = "Summarize the earlier role-play conversation";
  turns[2].textContent = "<state_snapshot>Historical details</state_snapshot>";
  const x = context(turns);
  const selection = x.compactor.preflightDom(x.compactor.planThoughtCleanup());
  assert.deepEqual(Array.from(selection.targetIds), ["thought"]);
  assert.ok(selection.protectedIds.includes("manual-prompt"));
  assert.ok(selection.protectedIds.includes("result"));
});

test("cloud summary marker inventory ignores disposal of temporary scope requests", () => {
  const x = context([]);
  const before = x.compactor.archiveInventory([
    {role:"user",text:"[[SUJIU_SCOPE_V2:12345678-aaaa]]"},
    {role:"model",text:"[[SUJIU_MEMORY_V1:12345678-aaaa]] Summary"}
  ],turn=>turn.role,turn=>turn.text);
  const after = x.compactor.archiveInventory([
    {role:"model",text:"[[SUJIU_MEMORY_V1:12345678-aaaa]] Summary"}
  ],turn=>turn.role,turn=>turn.text);
  assert.equal(before.compressionRequests, 1);
  assert.equal(after.compressionRequests, 0);
  assert.doesNotThrow(()=>x.compactor.verifyArchiveInventory(before,after));
});

test("native per-target failures are skipped; subsequent task and thinking are deleted while summary survives", async () => {
  const turns = [
    mockTurn("ordinary-user","user"),
    mockTurn("thought-bad","thought"),
    mockTurn("task","user"),
    mockTurn("thought-good","thought"),
    mockTurn("summary","model")
  ];
  turns[2].textContent = "[[SUJIU_SCOPE_V2:12345678-aaaa]]";
  turns[4].textContent = "[[SUJIU_MEMORY_V1:12345678-aaaa]] Persistent model reply";
  let active = null;
  const deleted = [];
  const overlay = {
    disabled: false,
    getAttribute(name) { return name === "aria-label" ? "Delete" : null; },
    querySelector() { return null; },
    dispatchEvent() {},
    click() {
      if (!active) throw new Error("No active menu turn");
      const index = turns.indexOf(active);
      if (index < 0) throw new Error("Turn already absent");
      deleted.push(active.id);
      turns.splice(index,1);
      active = null;
    }
  };
  for (const turn of turns) {
    turn.querySelector = selector => selector.includes("ms-chat-turn-options") ?
      { click() { active = turn; } } : null;
  }
  const document = {
    querySelectorAll(selector) {
      if (selector === "ms-chat-turn") return turns;
      return selector.includes(".cdk-overlay-container") ? [overlay] : [];
    },
    getElementById(id) { return turns.find(t=>t.id===id) || null; }
  };
  const liveEntries = () => turns.map(turn => {
    const type = turn.getAttribute("data-turn-type");
    const entry = new Array(33).fill(null);
    entry[0] = turn.textContent;
    entry[8] = type === "thought" ? "model" : type;
    entry[19] = type === "thought" ? 1 : 0;
    return entry;
  });
  const x = context(turns,{
    document, confirmResult:true,
    onConfirm() { turns[1].querySelector = () => null; },
    fetchResolvedPromptThread: async () => ({entries: liveEntries()}),
    extractResolvedTurns: entries => entries.map(entry => ({
      role: entry[8], text: entry[0], entry
    })),
    SaveMonitor: {
      attempts: 0,
      async waitForNativeSave() {
        return {observed:true,ok:true,status:200,attempts:1};
      }
    }
  });
  x.compactor.stage(turns[4].textContent,
    {totalRounds:1,olderRounds:0,recentRounds:1},
    {marker:"[[SUJIU_SCOPE_V2:12345678-aaaa]]"});
  await x.compactor.cleanupThoughts();
  assert.deepEqual(deleted,["thought-good","task"]);
  assert.ok(document.getElementById("summary"),"the model summary must survive");
  assert.ok(document.getElementById("thought-bad"),"unavailable thought is skipped");
  assert.ok(x.statuses.some(msg=>msg.includes("跳过 1 条")));
  assert.ok(!x.statuses.some(msg=>msg.includes("云端保存记录发生变化")),
    "removing the disposable task must not count as losing a model summary");
  assert.equal(x.compactor.pending, null,
    "staged review of the deleted synthetic task is cleared");
  assert.equal(x.state.branchInProgress,false);
});

test("orphaned plugin request after a failed generation is still disposable", () => {
  const turns = [
    mockTurn("ordinary-user","user"),mockTurn("ordinary-reply","model"),
    mockTurn("failed-task","user"),mockTurn("failed-thinking","thought")
  ];
  turns[2].textContent = "[[SUJIU_SCOPE_V2:12345678-eeee]]";
  const x = context(turns);
  const selection = x.compactor.preflightDom(x.compactor.planThoughtCleanup());
  assert.deepEqual(Array.from(selection.targetIds), ["failed-thinking","failed-task"]);
  assert.ok(selection.protectedIds.includes("ordinary-user"));
  assert.ok(selection.protectedIds.includes("ordinary-reply"));
});

test("last deletion save timestamp ignores targets skipped after preflight", () => {
  const sourceLoop = source.slice(source.indexOf("for (const id of preview.targetIds)"),
    source.indexOf("SaveMonitor.waitForNativeSave("));
  assert.match(sourceLoop,/const attemptStartedAt = Date.now\(\)/);
  assert.match(sourceLoop,/if \(result\?\.deleted\) \{[\s\S]*?lastDeletionStartedAt = attemptStartedAt/);
  assert.doesNotMatch(sourceLoop,/lastDeletionStartedAt = Date.now\(\)/);
});

test("cleanup accepts a saved conversation beginning with a marked orphaned model summary", () => {
  const turns = [
    mockTurn("summary","model"),
    mockTurn("old-u","user"),mockTurn("old-m","model"),
    mockTurn("middle-u","user"),mockTurn("middle-m","model"),
    mockTurn("latest-u","user"),mockTurn("latest-m","model")
  ];
  turns[0].textContent = "[[SUJIU_MEMORY_V1:12345678-aaaa]] Earlier conversation";
  const x=context(turns);
  const groups=x.compactor.collectGroups(turns);
  assert.equal(groups[0].kind,"memory");
  assert.equal(groups[0].hasModel,true);
  const plan=x.compactor.planManualCleanup(1);
  assert.equal(plan.totalRounds,3);
  assert.equal(plan.olderRounds,2);
  const chosen=x.compactor.preflightDom(plan);
  assert.deepEqual(Array.from(chosen.targetIds),["old-m","old-u","middle-m","middle-u"]);
  assert.ok(chosen.protectedIds.includes("summary"));
  assert.ok(chosen.protectedIds.includes("latest-u"));
  assert.ok(chosen.protectedIds.includes("latest-m"));
});

test("first orphan model with explicit memory marker remains pinned", () => {
  const turns=[
    mockTurn("legacy-model","model"),
    mockTurn("old-u","user"),mockTurn("old-m","model"),
    mockTurn("latest-u","user"),mockTurn("latest-m","model")
  ];
  turns[0].textContent="[[SUJIU_MEMORY_V1:12345678-abcd]] Old summary";
  const x=context(turns);
  const groups=x.compactor.collectGroups(turns);
  assert.equal(groups[0].kind,"memory");
  assert.equal(groups[0].hasModel,true);
  const preview=x.compactor.preflightDom(x.compactor.planManualCleanup(1));
  assert.deepEqual(Array.from(preview.targetIds),["old-m","old-u"]);
  assert.ok(preview.protectedIds.includes("legacy-model"));
});

test("leading thinking with marked model summary forms protected archive group", () => {
  const turns=[
    mockTurn("orphan-thought","thought"),
    mockTurn("orphan-summary","model"),
    mockTurn("old-u","user"),mockTurn("old-m","model"),
    mockTurn("latest-u","user"),mockTurn("latest-m","model")
  ];
  turns[1].textContent="[[SUJIU_MEMORY_V1:12345678-abcd]] Pinned summary";
  const x=context(turns);
  const groups=x.compactor.collectGroups(turns);
  assert.equal(groups[0].kind,"memory");
  assert.equal(groups[0].hasModel,true);
  assert.deepEqual(Array.from(groups[0].turns.map(turn=>turn.id)),
    ["orphan-thought","orphan-summary"]);
  const preview=x.compactor.preflightDom(x.compactor.planManualCleanup(1));
  assert.deepEqual(Array.from(preview.targetIds),["old-m","old-u"]);
  assert.ok(preview.protectedIds.includes("orphan-summary"));
});

test("RoundSplitter uses explicit marker to preserve first orphan model summary", () => {
  const rounds=vm.runInNewContext(roundsSource+"\nRoundSplitter",{});
  const turns=[
    {role:"model",text:"[[SUJIU_MEMORY_V1:12345678-abcd]] Previous summary"},
    {role:"user",text:"A"},{role:"model",text:"Reply A"},
    {role:"user",text:"B"},{role:"model",text:"Reply B"},
    {role:"user",text:"C"},{role:"model",text:"Reply C"}
  ];
  const result=rounds.partition(turns,1);
  assert.equal(result.totalRounds,3);
  assert.equal(result.olderRounds,2);
  assert.equal(result.pinnedMemories,1);
  assert.equal(result.older[0].text,turns[0].text);
  assert.equal(result.recent[0].text,"C");
});

test("all consecutive explicitly marked summaries survive cleanup", () => {
  const turns=[
    mockTurn("pre-u","user"),mockTurn("pre-m","model"),
    mockTurn("older-u","user"),mockTurn("older-m","model"),
    mockTurn("archive-1","model"),mockTurn("archive-2","model"),
    mockTurn("archive-3","model"),
    mockTurn("current-u","user"),mockTurn("current-m","model")
  ];
  turns[4].textContent="[[SUJIU_MEMORY_V1:12345678-aaaa]] First compressed memory";
  turns[5].textContent="[[SUJIU_MEMORY_V1:12345678-bbbb]] Second summary";
  turns[6].textContent="[[SUJIU_MEMORY_V1:12345678-cccc]] Third summary";
  const x=context(turns);
  const groups=x.compactor.collectGroups(turns);
  assert.equal(groups.filter(group=>group.kind==="memory").length,3);
  const plan=x.compactor.planManualCleanup(1);
  assert.equal(plan.totalRounds,3);
  assert.equal(plan.olderRounds,2);
  const preview=x.compactor.preflightDom(plan);
  assert.equal(preview.protectedSummaryCount,3);
  assert.deepEqual(Array.from(preview.targetIds),["pre-m","pre-u","older-m","older-u"]);
  for(const id of ["archive-1","archive-2","archive-3","current-u","current-m"])
    assert.ok(preview.protectedIds.includes(id),id+" must survive");
});

test("all marked archives interspersed with rounds remain pinned", () => {
  const turns=[
    mockTurn("archive-first","model"),
    mockTurn("old-u","user"),mockTurn("old-m","model"),
    mockTurn("archive-middle","model"),
    mockTurn("next-u","user"),mockTurn("next-m","model"),
    mockTurn("archive-last-a","model"),mockTurn("archive-last-b","model"),
    mockTurn("recent-u","user"),mockTurn("recent-m","model")
  ];
  turns[0].textContent="[[SUJIU_MEMORY_V1:12345678-aaaa]] A";
  turns[3].textContent="[[SUJIU_MEMORY_V1:12345678-bbbb]] B";
  turns[6].textContent="[[SUJIU_MEMORY_V1:12345678-cccc]] C";
  turns[7].textContent="[[SUJIU_MEMORY_V1:12345678-dddd]] D";
  const x=context(turns);
  const groups=x.compactor.collectGroups(turns);
  assert.equal(groups.filter(group=>group.kind==="memory").length,4);
  const plan=x.compactor.planManualCleanup(1);
  assert.equal(plan.totalRounds,3);
  const preview=x.compactor.preflightDom(plan);
  assert.equal(preview.protectedSummaryCount,4);
  assert.deepEqual(Array.from(preview.targetIds),["old-m","old-u","next-m","next-u"]);
  for(const id of ["archive-first","archive-middle","archive-last-a","archive-last-b"])
    assert.ok(preview.protectedIds.includes(id),id+" must survive");
});

test("all explicitly marked model archives are excluded from recent N rounds", () => {
  const rounds=vm.runInNewContext(roundsSource+"\nRoundSplitter",{});
  const turns=[
    {role:"user",text:"Old user"},{role:"model",text:"Old reply"},
    {role:"model",text:"[[SUJIU_MEMORY_V1:12345678-aaaa]] A"},
    {role:"model",text:"[[SUJIU_MEMORY_V1:12345678-bbbb]] B"},
    {role:"model",text:"[[SUJIU_MEMORY_V1:12345678-cccc]] C"},
    {role:"user",text:"Middle user"},{role:"model",text:"Middle reply"},
    {role:"model",text:"[[SUJIU_MEMORY_V1:12345678-dddd]] D"},
    {role:"user",text:"Recent user"},{role:"model",text:"Recent reply"}
  ];
  const split=rounds.partition(turns,1);
  assert.equal(split.totalRounds,3);
  assert.equal(split.olderRounds,2);
  assert.equal(split.pinnedMemories,4);
  for (const text of ["[[SUJIU_MEMORY_V1:12345678-aaaa]] A",
    "[[SUJIU_MEMORY_V1:12345678-bbbb]] B","[[SUJIU_MEMORY_V1:12345678-cccc]] C","[[SUJIU_MEMORY_V1:12345678-dddd]] D"])
    assert.ok(split.older.some(t=>t.text===text),text+" must be included in old history context");
  assert.equal(split.recent[0].text,"Recent user");
});

test("marked standalone model summaries are protected at every position", () => {
  const turns=[
    mockTurn("u1","user"),mockTurn("m1","model"),
    mockTurn("archive-1","model"),
    mockTurn("u2","user"),mockTurn("m2","model"),
    mockTurn("archive-2","model"),
    mockTurn("archive-3","model"),
    mockTurn("u3","user"),mockTurn("m3","model")
  ];
  turns[2].textContent="[[SUJIU_MEMORY_V1:12345678-aaaa]] A";
  turns[3].textContent="Just a normal user question";
  turns[5].textContent="[[SUJIU_MEMORY_V1:12345678-bbbb]] B";
  turns[6].textContent="[[SUJIU_MEMORY_V1:12345678-cccc]] C";
  const x=context(turns);
  const plan=x.compactor.planManualCleanup(1);
  const p=x.compactor.preflightDom(plan);
  for(const id of ["archive-1","archive-2","archive-3"])
    assert.ok(!p.targetIds.includes(id),id+" cannot be deleted");
  assert.equal(p.protectedSummaryCount,3);
});

test("explicitly marked model archives survive even with intervening thought chunks", () => {
  const turns=[
    mockTurn("old-u","user"),mockTurn("old-m","model"),
    mockTurn("archive-think-1","thought"),mockTurn("archive-a","model"),
    mockTurn("archive-think-2","thought"),mockTurn("archive-b","model"),
    mockTurn("latest-u","user"),mockTurn("latest-m","model")
  ];
  turns[3].textContent="[[SUJIU_MEMORY_V1:12345678-aaaa]] Model archive one";
  turns[5].textContent="[[SUJIU_MEMORY_V1:12345678-bbbb]] Model archive two";
  const x=context(turns);
  const groups=x.compactor.collectGroups(turns);
  const preview=x.compactor.preflightDom(x.compactor.planManualCleanup(1));
  assert.equal(groups.filter(g=>g.kind==="memory").length,2);
  assert.equal(preview.protectedSummaryCount,2);
  assert.ok(preview.protectedIds.includes("archive-a"));
  assert.ok(preview.protectedIds.includes("archive-b"));
  assert.ok(!preview.targetIds.includes("archive-a"));
  assert.ok(!preview.targetIds.includes("archive-b"));
});

test("native deletion checks marker instead of inferring protection from position", () => {
  const code=source.slice(source.indexOf("async function deleteOne("),
    source.indexOf("async function commit("));
  assert.match(code,/type !== "thought" && isProtectedTurn\(turn\)/);
  assert.doesNotMatch(code,/previous !== "user"/);
});

