import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const history = await readFile(new URL("../src/40-history.js", import.meta.url), "utf8");
const recoveryCode = history.slice(0, history.indexOf("    function extractStateSnapshot("));
const compression = await readFile(new URL("../src/30-compression.js", import.meta.url), "utf8");
const network = await readFile(new URL("../src/20-network.js", import.meta.url), "utf8");

function turn(role, text) { return { role, text }; }
function fixture(entries) {
  const staged = [];
  const context = {
    Branching: { getCurrentPromptId: () => "test-prompt" },
    fetchResolvedPromptThread: async () => ({ entries }),
    extractResolvedTurns: value => value,
    assertSupportedEntries() {},
    RoundSplitter: { partition(turns, keep) {
      assert.equal(keep, 1);
      assert.equal(turns.length, 4);
      return { totalRounds: 2, olderRounds: 1, recentRounds: 1 };
    }},
    InPlaceCompaction: { stage: (...args) => staged.push(args) },
    userSettings: { keepRounds: 1 },
    log: { ui() {} }
  };
  const recover = vm.runInNewContext(recoveryCode + "\nrecoverLatestCompletedSummary", context);
  return { recover, staged };
}

test("manual recovery reads the last saved summary and stages deletion only after explicit request", async () => {
  const turns = [
    turn("user", "early"), turn("model", "early reply"),
    turn("user", "recent"), turn("model", "recent reply"),
    turn("user", "Summarize\nOnly summarize the completed dialogue BEFORE the last 1 full user/model exchanges."),
    turn("model", "Saved snapshot")
  ];
  const { recover, staged } = fixture(turns);
  assert.equal(staged.length, 0);
  await recover();
  assert.equal(staged.length, 1);
  assert.equal(staged[0][0], "Saved snapshot");
  assert.equal(staged[0][1].olderRounds, 1);
});

test("manual recovery rejects arbitrary recent messages rather than deleting history", async () => {
  const turns = [
    turn("user", "old"), turn("model", "old reply"),
    turn("user", "recent"), turn("model", "recent reply"),
    turn("user", "normal question"), turn("model", "normal response")
  ];
  const { recover, staged } = fixture(turns);
  await assert.rejects(recover(), /不是素酒生成的摘要指令/);
  assert.equal(staged.length, 0);
});

test("manual recovery refuses incomplete model output", async () => {
  const turns = [
    turn("user", "old"), turn("model", "old reply"),
    turn("user", "recent"), turn("model", "recent reply"),
    turn("user", "Only summarize the completed dialogue BEFORE the last 1 full user/model exchanges."),
    turn("model", "")
  ];
  const { recover, staged } = fixture(turns);
  await assert.rejects(recover(), /完整模型摘要回复/);
  assert.equal(staged.length, 0);
});

test("XHR request observer provides bodyText required by compression matching", () => {
  assert.match(network, /emitNetworkEvent\(Config\.EVENTS\.REQUEST,\s*\{\s*url: absoluteUrl,\s*headers,\s*bodyText\s*\}/);
});

test("compression response listening has no fixed timeout, and recover is separate", () => {
  assert.doesNotMatch(compression, /waitForCompressionRequestCapture|timeoutMs = 12000/);
  assert.match(compression, /async recover\(\)/);
  assert.match(compression, /stopTracking\(\)/);
  assert.doesNotMatch(compression, /prepareInPlaceCompaction\(snapshot\)/);
});
