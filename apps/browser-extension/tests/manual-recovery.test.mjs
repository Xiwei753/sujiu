import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const history = await readFile(new URL("../src/40-history.js", import.meta.url), "utf8");
const recoveryCode = history.slice(0, history.indexOf("    function extractStateSnapshot("));
const taskSource = await readFile(new URL("../src/25-compaction-task.js", import.meta.url), "utf8");
const compression = await readFile(new URL("../src/30-compression.js", import.meta.url), "utf8");
const network = await readFile(new URL("../src/20-network.js", import.meta.url), "utf8");
const roundSource = await readFile(new URL("../src/35-rounds.js", import.meta.url), "utf8");
const stateSource = await readFile(new URL("../src/00-state.js", import.meta.url), "utf8");

const makeTurn = (role, text, thought = false) =>
  ({ role, text, entry: thought ? Object.assign(Array(33).fill(null), { 19: 1 }) : null });
const pairs = n => Array.from({ length: n }, (_, i) =>
  [makeTurn("user", "Question " + i), makeTurn("model", "Answer " + i)]).flat();

function fixture(entries, { keepRounds = 1, stored = null } = {}) {
  const staged = [];
  const context = {
    Branching: { getCurrentPromptId: () => "test-prompt" },
    fetchResolvedPromptThread: async () => ({ entries }),
    extractResolvedTurns: value => value,
    assertSupportedEntries() {},
    UI: { findSendButton: () => null },
    isStopControl: () => false,
    isThoughtOnlyTurn: turn => turn?.entry?.[19] === 1,
    InPlaceCompaction: { stage: (...args) => staged.push(args) },
    userSettings: { keepRounds },
    log: { ui() {} },
    localStorage: {
      getItem() { return stored ? JSON.stringify(stored) : null; },
      setItem() {}
    }
  };
  context.CompactionTask = vm.runInNewContext(taskSource + "\nCompactionTask", context);
  context.RoundSplitter = vm.runInNewContext(roundSource + "\nRoundSplitter", context);
  const methods = vm.runInNewContext(
    recoveryCode + "\n({findSavedSummary,recoverLatestCompletedSummary})", context);
  return { ...methods, staged, CompactionTask: context.CompactionTask };
}

test("legacy v0.2.2 summary is recognized, even with thought records after visible reply", async () => {
  const turns = [
    ...pairs(2),
    makeTurn("user", "Summarize\nOnly summarize the completed dialogue BEFORE the last 1 full user/model exchanges."),
    makeTurn("model", "The older event was resolved."),
    makeTurn("model", "Private thinking, not part of the summary", true)
  ];
  const x = fixture(turns);
  await x.recoverLatestCompletedSummary();
  assert.equal(x.staged.length, 1);
  assert.equal(x.staged[0][0], "The older event was resolved.");
  assert.equal(x.staged[0][1].olderRounds, 1);
  assert.equal(x.staged[0][2].legacy, true);
});

test("custom user prompt with generated marker selects the embedded keepRounds, even if settings change", async () => {
  const id = "12345678-1234-1234-1234-abcdef123456";
  const marker = "[[SUJIU_SUMMARY_V1:" + id + ":KEEP=20]]";
  const turns = [
    ...pairs(23),
    makeTurn("user", "Custom Chinese instruction with no fixed English sentence.\n" + marker),
    makeTurn("model", "按时间线总结：事件已经结束。")
  ];
  const x = fixture(turns, {
    keepRounds: 8,
    stored: { promptId: "test-prompt", marker, keepRounds: 20 }
  });
  await x.recoverLatestCompletedSummary();
  assert.equal(x.staged[0][1].olderRounds, 3);
  assert.equal(x.staged[0][1].recentRounds, 20);
  assert.equal(x.staged[0][2].marker, marker);
});

test("an unrelated new user prompt after the summary rejects destructive recovery", async () => {
  const turns = [
    ...pairs(2),
    makeTurn("user", "Only summarize the completed dialogue BEFORE the last 1 full user/model exchanges."),
    makeTurn("model", "Summary"),
    makeTurn("user", "Normal newer question"),
    makeTurn("model", "Normal newer response")
  ];
  const x = fixture(turns);
  await assert.rejects(x.recoverLatestCompletedSummary(), /最新用户消息没有素酒摘要任务标识/);
  assert.equal(x.staged.length, 0);
});

test("mismatched task marker is rejected", async () => {
  const marker = "[[SUJIU_SUMMARY_V1:12345678-abcd:KEEP=1]]";
  const turns = [...pairs(2), makeTurn("user", marker), makeTurn("model", "Finished")];
  const x = fixture(turns, {
    stored: { promptId: "test-prompt", marker: "[[SUJIU_SUMMARY_V1:87654321-abcd:KEEP=1]]" }
  });
  await assert.rejects(x.recoverLatestCompletedSummary(), /本机最近一次生成任务不同/);
  assert.equal(x.staged.length, 0);
});

test("incomplete saved summary is rejected", async () => {
  const turns = [
    ...pairs(2),
    makeTurn("user", "Only summarize the completed dialogue BEFORE the last 1 full user/model exchanges.")
  ];
  const x = fixture(turns);
  await assert.rejects(x.recoverLatestCompletedSummary(), /摘要后尚无完整的模型回复/);
  assert.equal(x.staged.length, 0);
});

test("summary marker is parseable and base prompt remains editable", () => {
  const x = fixture([]);
  const task = x.CompactionTask.create("test-prompt", 20);
  const prompt = x.CompactionTask.compose("自定义文学摘要指令", task);
  const parsed = x.CompactionTask.parsePrompt(prompt);
  assert.equal(parsed.keepRounds, 20);
  assert.equal(parsed.marker, task.marker);
  assert.ok(prompt.startsWith("自定义文学摘要指令"));
  assert.equal(x.CompactionTask.parsePrompt("Unrelated message"), null);
});

test("default memory prompt distinguishes archived events from the actual current scene", () => {
  const match = stateSource.match(/\n            FULL: ("(?:\\.|[^"\\])*"),\n            SNIPPET:/);
  assert.ok(match, "default prompt literal exists");
  const defaultPrompt = JSON.parse(match[1]);
  assert.match(defaultPrompt, /压缩截点/);
  assert.match(defaultPrompt, /已发生的关键事件/);
  assert.doesNotMatch(defaultPrompt, /current situation before/);
});

test("XHR request observer provides bodyText for compression matching", () => {
  assert.match(network, /emitNetworkEvent\(Config\.EVENTS\.REQUEST,\s*\{\s*url: absoluteUrl,\s*headers,\s*bodyText\s*\}/);
});

test("compression has no fixed capture timeout and permits manual recovery", () => {
  assert.doesNotMatch(compression, /waitForCompressionRequestCapture|timeoutMs = 12000/);
  assert.match(compression, /async recover\(\)/);
  assert.match(compression, /stopTracking\(\)/);
});
