import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const rounds = await readFile(new URL("../src/35-rounds.js", import.meta.url), "utf8");
const source = await readFile(new URL("../src/28-request-scope.js", import.meta.url), "utf8");
const network = await readFile(new URL("../src/20-network.js", import.meta.url), "utf8");
const marker = "[[SUJIU_SCOPE_V2:abcdef12-1234]]";
const make = (role, text) => [[[null, text]], role];
const context = {
  safeParseJSON: JSON.parse,
  RoundSplitter: vm.runInNewContext(rounds + "\nRoundSplitter")
};
const Scope = vm.runInNewContext(source + "\nRequestScope", context);

const initial = [
  make("user", "old message 1"), make("model", "old response 1"),
  make("user", "old message 2"), make("model", "old response 2"),
  make("user", "recent message"), make("model", "recent response"),
  make("user", "Summarize whole conversation\n" + marker)
];
const wire = JSON.stringify(["options", initial, { modelOptions: ["keep"] }]);

test("all old and recent original turns are transmitted for the summary", () => {
  const output = Scope.prepare(wire, marker, 1);
  assert.equal(output.olderRounds, 2);
  assert.equal(output.recentRounds, 1);
  assert.equal(output.totalRounds, 3);
  assert.equal(output.transmittedRounds, 3);
  assert.equal(output.bodyText, wire, "Google's native request is not rewritten");
  const body = JSON.parse(output.bodyText);
  assert.equal(body[1].length, 7);
  assert.equal(body[2].modelOptions[0], "keep");
  for(const fragment of ["old message 1","old message 2","recent message","recent response",marker])
    assert.ok(output.bodyText.includes(fragment));
});

test("94-round history transmits the entire 94 rounds while preserving last 20 for cleanup", () => {
  const history = Array.from({length:94}, (_,i)=>[
    make("user","user "+i), make("model","model "+i)
  ]).flat();
  const body = JSON.stringify(["options", [...history,make("user",marker)]]);
  const result = Scope.prepare(body, marker, 20);
  assert.equal(result.olderRounds,74);
  assert.equal(result.recentRounds,20);
  assert.equal(result.transmittedRounds,94);
  assert.equal(JSON.parse(result.bodyText)[1].length,189);
  assert.ok(result.bodyText.includes("user 93"));
  assert.ok(result.bodyText.includes("model 93"));
});

test("unexpected native private RPC structures fail safely rather than rewriting content", () => {
  assert.throws(() => Scope.prepare(JSON.stringify(["options", {}, { marker }]), marker, 1), /格式无法验证/);
  assert.throws(() => Scope.prepare(JSON.stringify(["options", [
    make("user", "old"), make("model", "reply"),
    [[null], "tool"], make("user", marker)
  ]]), marker, 1), /无法识别/);
  assert.throws(() => Scope.prepare(JSON.stringify(["options", [
    ...initial.slice(0,-1), make("user","other")
  ], {marker}]), marker, 1), /本次摘要指令/);
  assert.equal(Scope.prepare(wire,"[[SUJIU_SCOPE_V2:other-task]]",1),null);
});

test("duplicate recent content in native request metadata is not altered", () => {
  const mirrored = JSON.stringify(["options",initial,{ mirror: initial[4][0]}]);
  const result = Scope.prepare(mirrored,marker,1);
  assert.equal(result.bodyText,mirrored);
});

test("both XHR and fetch validate the full request but send the original bytes", () => {
  assert.equal((network.match(/RequestScope\.prepare\(/g)||[]).length,2);
  assert.match(network, /const response = await originalFetch\(input, init\)/);
  assert.match(network, /return originalSend\.apply\(this, arguments\)/);
  assert.doesNotMatch(network,/body = scope\.bodyText|new Request\(request, \{ body: scope\.bodyText \}\)/);
  assert.match(network,/摘要请求包含全部/);
  assert.doesNotMatch(network,/最近 .* 轮未发送/);
});
