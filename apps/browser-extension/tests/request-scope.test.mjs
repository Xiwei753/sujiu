import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const rounds = await readFile(new URL("../src/35-rounds.js", import.meta.url), "utf8");
const source = await readFile(new URL("../src/28-request-scope.js", import.meta.url), "utf8");
const network = await readFile(new URL("../src/20-network.js", import.meta.url), "utf8");
const marker = "[[SUJIU_SCOPE_V2:abcdef12-1234]]";
const make = (role, text) => [[[null, text]], role];
const parse = text => JSON.parse(text);
const context = {
  safeParseJSON: parse,
  RoundSplitter: vm.runInNewContext(rounds + "\nRoundSplitter")
};
const Scope = vm.runInNewContext(source + "\nRequestScope", context);
const initial = [
  make("user", "old message 1"), make("model", "old response 1"),
  make("user", "old message 2"), make("model", "old response 2"),
  make("user", "private current message"), make("model", "private current response"),
  make("user", "总结已提供历史\n" + marker)
];
const wire = JSON.stringify(["other", initial, { modelOptions: ["keep"] }]);
test("model request contains full older turns and summary but never protected recent turns", () => {
  const output = Scope.prepare(wire, marker, 1);
  assert.equal(output.olderRounds, 2);
  assert.equal(output.recentRounds, 1);
  assert.equal(output.totalRounds, 3);
  const body = JSON.parse(output.bodyText);
  assert.equal(body[1].length, 5);
  assert.equal(body[2].modelOptions[0], "keep");
  assert.match(output.bodyText, /old message 1/);
  assert.match(output.bodyText, /old message 2/);
  assert.match(output.bodyText, /SUJIU_SCOPE_V2/);
  assert.doesNotMatch(output.bodyText, /private current message|private current response/);
  assert.equal(JSON.parse(wire)[1].length, 7, "original request is unchanged");
});

test("refuses unsafe private RPC schemas instead of leaking full history", () => {
  assert.throws(() => Scope.prepare(JSON.stringify(["other", {}, {}]), marker, 1), /格式无法验证/);
  assert.throws(() => Scope.prepare(JSON.stringify(["other", [
    make("user", "old"), make("model", "reply"),
    [[null], "tool"], make("user", marker)
  ]]), marker, 1), /不兼容/);
  assert.throws(() => Scope.prepare(JSON.stringify(["other", [
    ...initial.slice(0, -1), make("user", "ordinary")
  ]]), marker, 1), /本次压缩指令/);
  assert.equal(Scope.prepare(wire, "[[SUJIU_SCOPE_V2:other-task]]", 1), null);
});

test("scope field outside history containing recent content rejects unsafe request", () => {
  const dup = JSON.stringify(["other", initial, { mirror: initial[4][0] }]);
  assert.throws(() => Scope.prepare(dup, marker, 1), /近期内容存在于请求的其他字段/);
});

test("both XHR and fetch are routed through the old-only scope before native network send", () => {
  const uses = network.match(/RequestScope\.prepare\(/g) || [];
  assert.equal(uses.length, 2);
  assert.match(network, /originalFetch\(\s*shouldLogGenerate && actualRequest !== request/);
  assert.match(network, /body = scope\.bodyText/);
});
