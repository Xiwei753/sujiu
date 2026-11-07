import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const profilesSource = await readFile(new URL("../src/75-prompt-profiles.js", import.meta.url), "utf8");
const panelSource = await readFile(new URL("../src/80-prompt-panel.js", import.meta.url), "utf8");

function instantiate(legacy) {
  const storage = new Map();
  if (legacy !== undefined) storage.set("sujiu:aistudio:prompt-presets:v2", JSON.stringify(legacy));
  const localStorage = {
    getItem: key => storage.get(key) ?? null,
    setItem: (key, val) => storage.set(key, String(val))
  };
  const context = { localStorage };
  const read = vm.runInNewContext(profilesSource + "\n" +
    panelSource.replace("    let data = load();",
      "    let data = load(); globalThis.__panelMigrated = data;") +
    "\nPromptPanel", context);
  return { instance: read, storage, migrated: context.__panelMigrated };
}

test("migrates existing five-section preset once and keeps its original backup", () => {
  const old = { version: 2, active: "我的剧情", presets: { "我的剧情": {
    fields: { system: "固定规则", character: "主角设定", author: "临时草稿" },
    enabled: { author: false }, order: ["character","system","author"]
  }}};
  const { storage, migrated } = instantiate(old);
  const key = "sujiu:aistudio:prompt-presets:v3";
  const backup = "sujiu:aistudio:prompt-presets:backup:v2";
  assert.equal(migrated.version, 3);
  assert.equal(migrated.active, "我的剧情");
  assert.match(migrated.presets["我的剧情"].text, /^\[角色设定\]\n主角设定/);
  assert.match(migrated.presets["我的剧情"].text, /固定规则/);
  assert.doesNotMatch(migrated.presets["我的剧情"].text, /临时草稿/);
  assert.equal(migrated.presets["我的剧情"].legacyDisabled.author, "临时草稿");
  assert.deepEqual(JSON.parse(storage.get(backup)), old);
  assert.deepEqual(JSON.parse(storage.get(key)), JSON.parse(JSON.stringify(migrated)));
  assert.deepEqual(JSON.parse(storage.get("sujiu:aistudio:prompt-presets:v2")), old);
});

test("new v3 presets are authoritative and do not get overwritten by old v2 data", () => {
  const old = {active:"Old",presets:{Old:{system:"old"}}};
  const {storage} = instantiate(old);
  storage.set("sujiu:aistudio:prompt-presets:v3", JSON.stringify({
    version:3, active:"New", presets:{New:{text:"fresh text"}}
  }));
  const context = {localStorage: {
    getItem: key => storage.get(key) ?? null,
    setItem: (key,val) => storage.set(key,String(val))
  }};
  vm.runInNewContext(profilesSource + "\n" +
    panelSource.replace("    let data = load();",
      "    let data = load(); globalThis.__panelMigrated = data;") +
    "\nPromptPanel", context);
  assert.equal(context.__panelMigrated.active,"New");
  assert.equal(context.__panelMigrated.presets.New.text,"fresh text");
});

test("panel has one system prompt textarea and independent compression template", () => {
  assert.match(panelSource,/role\("system-prompt", "textarea"/);
  assert.match(panelSource,/role\("summary-prompt", "textarea"/);
  assert.doesNotMatch(panelSource,/dataset\.move|dataset\.enable|data-field|renderFields/);
  assert.match(panelSource,/role === "system-prompt"/);
  assert.match(panelSource,/current\(\)\.text =/);
  assert.match(panelSource,/PromptProfiles\.compile\(current\(\)\)/);
});
