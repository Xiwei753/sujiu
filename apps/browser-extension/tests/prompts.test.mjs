import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(new URL("../src/75-prompt-profiles.js", import.meta.url), "utf8");
const profiles = vm.runInNewContext(source + "\nPromptProfiles", {});

test("five prompt fields compile in a stable order", () => {
  const p = profiles.blank();
  p.fields.system = "Follow the format";
  p.fields.persona = "User is a traveler";
  p.fields.world = "A fantasy world";
  const text = profiles.compile(p);
  assert.ok(text.includes("[系统提示词]\\nFollow the format"));
  assert.ok(text.indexOf("[系统提示词]") < text.indexOf("[世界设定]"));
  assert.ok(text.indexOf("[世界设定]") < text.indexOf("[用户人物卡]"));
  p.enabled.world = false;
  const withoutWorld = profiles.compile(p);
  assert.ok(!withoutWorld.includes("A fantasy world"));
});

test("reordering takes effect and leaves the input immutable", () => {
  const before = profiles.blank();
  before.fields.system = "A";
  before.fields.character = "B";
  const after = profiles.shift(before, "character", -1);
  assert.equal(after.order[0], "character");
  assert.equal(before.order[0], "system");
  assert.ok(profiles.compile(after).startsWith("[角色设定]"));
});

test("legacy presets migrate and export/import normalization round-trips", () => {
  const legacy = { active: "Old", presets: { Old: { system: "one", persona: "two" } } };
  const upgraded = profiles.normalize(legacy);
  assert.equal(upgraded.version, 2);
  assert.equal(upgraded.presets.Old.fields.system, "one");
  const imported = profiles.normalize(JSON.parse(JSON.stringify(upgraded)));
  assert.equal(imported.presets.Old.fields.persona, "two");
});

test("malformed and unsafe preset names are ignored", () => {
  const input = JSON.parse('{"active":"__proto__","presets":{"__proto__":{"system":"bad"},"valid":{"system":"good"}}}');
  const normalized = profiles.normalize(input);
  assert.deepEqual(Object.keys(normalized.presets), ["valid"]);
  assert.equal(normalized.active, "valid");
  assert.equal(normalized.presets.valid.fields.system, "good");
});
