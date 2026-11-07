import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(new URL("../src/75-prompt-profiles.js", import.meta.url), "utf8");
const profiles = vm.runInNewContext(source + "\nPromptProfiles", {});

test("new presets contain one editable system prompt without artificial section labels", () => {
  const preset = profiles.blank();
  assert.equal(profiles.compile(preset), "");
  preset.text = "Follow the character and world rules\nNever alter the narrative";
  assert.equal(profiles.compile(preset), preset.text);
  const normalized = profiles.normalize({ active: "Default", presets: { Default: preset } });
  assert.equal(normalized.version, 3);
  assert.equal(normalized.active, "Default");
  assert.equal(normalized.presets.Default.text, preset.text);
});

test("old five-section presets merge in the original active order without lost text", () => {
  const legacy = {
    active: "长篇", presets: {
      "长篇": {
        fields: {
          system: "Always follow the style",
          character: "Character A",
          world: "Fantasy world",
          persona: "Player name",
          author: "Tone rules"
        },
        enabled: { system: true, character: true, world: true, persona: true, author: true },
        order: ["persona","author","system","world","character"]
      }
    }
  };
  const data = profiles.normalize(legacy);
  assert.equal(data.version, 3);
  assert.deepEqual(Object.keys(data.presets), ["长篇"]);
  const text = data.presets["长篇"].text;
  for (const part of Object.values(legacy.presets["长篇"].fields))
    assert.ok(text.includes(part));
  const order = ["[用户人物卡]","[作者附注]","[系统提示词]","[世界设定]","[角色设定]"];
  assert.deepEqual(order, order.slice().sort((a,b)=>text.indexOf(a)-text.indexOf(b)));
  assert.ok(text.includes("[角色设定]\nCharacter A"));
});

test("disabled historical fields are retained as export metadata, not silently activated", () => {
  const original = {
    active:"P", presets:{ P: {
      fields: { system: "active", world: "inactive draft" },
      enabled: { system: true, world: false }
    }}
  };
  const converted = profiles.normalize(original);
  assert.ok(converted.presets.P.text.includes("active"));
  assert.ok(!converted.presets.P.text.includes("inactive draft"));
  assert.equal(converted.presets.P.legacyDisabled.world, "inactive draft");
  const exported = JSON.parse(JSON.stringify(converted));
  const imported = profiles.normalize(exported);
  assert.equal(imported.presets.P.text, converted.presets.P.text);
  assert.equal(imported.presets.P.legacyDisabled.world, "inactive draft");
});

test("older direct-field presets and multiple named presets migrate individually", () => {
  const legacy = {active: "Old", presets: {
    Old: {system:"one",persona:"two"},
    New: {fields:{character:"three"},order:["character","system"]}
  }};
  const upgraded = profiles.normalize(legacy);
  assert.equal(upgraded.version, 3);
  assert.ok(upgraded.presets.Old.text.includes("[用户人物卡]\ntwo"));
  assert.ok(upgraded.presets.New.text.startsWith("[角色设定]\nthree"));
  const secondPass = profiles.normalize(JSON.parse(JSON.stringify(upgraded)));
  assert.equal(secondPass.presets.Old.text, upgraded.presets.Old.text);
});

test("migration never truncates content that fitted five historical fields", () => {
  const long = "字".repeat(100000);
  const fields = Object.fromEntries(["system","character","world","persona","author"]
    .map(id=>[id,long]));
  const result = profiles.normalize({active:"大纲",presets:{大纲:{fields}}});
  assert.equal(result.presets["大纲"].text.split("字").length-1, 500000);
});

test("malformed and unsafe preset names are ignored", () => {
  const input = JSON.parse('{"active":"__proto__","presets":{"__proto__":{"text":"bad"},"valid":{"text":"good"}}}');
  const normalized = profiles.normalize(input);
  assert.deepEqual(Object.keys(normalized.presets), ["valid"]);
  assert.equal(normalized.active, "valid");
  assert.equal(normalized.presets.valid.text, "good");
});
