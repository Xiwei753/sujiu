import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const history = await readFile(new URL("../src/40-history.js", import.meta.url), "utf8");
const taskSource = await readFile(new URL("../src/25-compaction-task.js", import.meta.url), "utf8");
const roundSource = await readFile(new URL("../src/35-rounds.js", import.meta.url), "utf8");
const stateSource = await readFile(new URL("../src/00-state.js", import.meta.url), "utf8");
const recoveryCode = history.slice(0, history.indexOf("    function extractStateSnapshot("));
const marker = "[[SUJIU_SCOPE_V2:abcd1234-0000]]";
const memory = "[[SUJIU_MEMORY_V1:abcd1234-0000]]";
const pairs = count => Array.from({ length: count }, (_, i) =>
  [{ role: "user", text: "u" + i }, { role: "model", text: "m" + i }]).flat();

function setup(turns, storageState = {}) {
  const staged = [];
  const context = {
    localStorage: {
      getItem: () => JSON.stringify(storageState),
      setItem() {}
    },
    Branching: { getCurrentPromptId: () => "chat-1" },
    fetchResolvedPromptThread: async () => ({ entries: turns }),
    extractResolvedTurns: entries => entries,
    assertSupportedEntries() {},
    UI: { findSendButton: () => null },
    isStopControl: () => false,
    isThoughtOnlyTurn: turn => Boolean(turn?.thought),
    InPlaceCompaction: { stage: (...args) => staged.push(args) },
    log: { ui() {} },
  };
  context.CompactionTask = vm.runInNewContext(taskSource + "\nCompactionTask", context);
  context.RoundSplitter = vm.runInNewContext(roundSource + "\nRoundSplitter", context);
  const recover = vm.runInNewContext(recoveryCode + "\nrecoverLatestCompletedSummary", context);
  return { recover, staged, task: context.CompactionTask };
}

test("reads only a verified old-only model summary and preserves protected rounds", async () => {
  const turns = [...pairs(3), { role: "user", text: "Summarize\n" + marker },
    { role: "model", text: memory + "\nPast scenes are complete." },
    { role: "model", text: "hidden reasoning", thought: true }];
  const x = setup(turns, { promptId: "chat-1", marker, keepRounds: 2, totalRounds: 3,
    olderRounds: 1, scopeVerified: true });
  await x.recover();
  assert.equal(x.staged.length, 1);
  assert.equal(x.staged[0][0], memory + "\nPast scenes are complete.");
  assert.equal(x.staged[0][1].olderRounds, 1);
  assert.equal(x.staged[0][1].recentRounds, 2);
});

test("refuses cleanup when the generated request was not verified to exclude protected turns", async () => {
  const turns = [...pairs(3), { role: "user", text: marker }, { role: "model", text: memory + "\nsummary" }];
  const x = setup(turns, { promptId: "chat-1", marker, keepRounds: 2, totalRounds: 3 });
  await assert.rejects(x.recover(), /无法确认本次模型请求已经裁掉近期对话/);
  assert.equal(x.staged.length, 0);
});

test("detects changed cloud history instead of deleting old records", async () => {
  const turns = [...pairs(2), { role: "user", text: marker }, { role: "model", text: memory + "\nsummary" }];
  const x = setup(turns, { promptId: "chat-1", marker, keepRounds: 1, totalRounds: 3,
    scopeVerified: true });
  await assert.rejects(x.recover(), /Google 保存的原聊天轮数与压缩前不一致/);
  assert.equal(x.staged.length, 0);
});

test("rejects a normal question after the summary", async () => {
  const turns = [...pairs(3), { role: "user", text: marker }, { role: "model", text: "summary" },
    { role: "user", text: "next story event" }, { role: "model", text: "new event" }];
  const x = setup(turns, { promptId: "chat-1", marker, keepRounds: 2, totalRounds: 3,
    scopeVerified: true });
  await assert.rejects(x.recover(), /最新用户消息没有素酒摘要任务标识/);
});

test("summary command contains neither protected count nor last-round instructions", () => {
  const x = setup([]);
  const task = x.task.create("chat-1", 20);
  const prompt = x.task.compose("仅总结收到的历史", task);
  assert.equal(x.task.parsePrompt(prompt)?.marker, task.marker);
  assert.ok(!prompt.includes("20"));
  assert.doesNotMatch(prompt, /recent|最近.*轮|保留.*轮/i);
});

test("default prompt summarizes only the supplied story chronology", () => {
  const m = stateSource.match(/\n            FULL: ("(?:\\.|[^"\\])*"),\n            SNIPPET:/);
  assert.ok(m);
  const prompt = JSON.parse(m[1]);
  assert.match(prompt, /实际提供的历史记录/);
  assert.doesNotMatch(prompt, /最近保留轮次|指定的较早轮次|最新剧情由保留/);
});
