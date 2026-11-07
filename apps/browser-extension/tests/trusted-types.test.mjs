import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(new URL("../src/80-prompt-panel.js", import.meta.url), "utf8");

class StrictNode {
  constructor(tag = "div") {
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.attributes = {};
    this.style = {};
    this.dataset = {};
    this.hidden = false;
    this.value = "";
    this.checked = false;
    this.id = "";
  }
  set innerHTML(_value) { throw new TypeError("This document requires TrustedHTML assignment"); }
  setAttribute(name, value) {
    this.attributes[name] = String(value);
    if (name === "id") this.id = String(value);
    if (name.startsWith("data-")) this.dataset[name.slice(5).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = String(value);
  }
  getAttribute(name) { return this.attributes[name] ?? null; }
  append(...children) { this.children.push(...children); }
  appendChild(node) { this.append(node); return node; }
  replaceChildren(...children) { this.children = children; }
  addEventListener() {}
  attachShadow() {
    this.shadowRoot = new StrictNode("shadow");
    return this.shadowRoot;
  }
  querySelector(selector) {
    const match = selector.match(/^\[([^=]+)="([^"]+)"\]$/);
    if (!match) return null;
    const [_, key, value] = match;
    const datasetKey = key.startsWith("data-")
      ? key.slice(5).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase()) : null;
    return this.find(node => node.getAttribute?.(key) === value ||
      (datasetKey && node.dataset?.[datasetKey] === value));
  }
  getElementById(id) { return this.find(node => node.id === id); }
  find(predicate) {
    for (const child of this.children) {
      if (predicate(child)) return child;
      const nested = child.find?.(predicate);
      if (nested) return nested;
    }
    return null;
  }
}

function setup() {
  const document = {
    body: new StrictNode("body"),
    readyState: "complete",
    createElement: tag => new StrictNode(tag),
    createTextNode: text => ({ textContent: String(text) }),
    getElementById(id) { return this.body.getElementById(id); }
  };
  const context = {
    document,
    localStorage: { getItem: () => null, setItem: () => {} },
    state: { compressionInProgress: false, branchInProgress: false },
    userSettings: { keepRounds: 8 },
    PerformanceTuning: { enabled: true, setEnabled(value) { this.enabled = Boolean(value); return this.enabled; } },
    getCompressPrompt: () => "Summary",
    setInterval: () => 1,
    console: { info: () => {} }
  };
  vm.runInNewContext(source + "\nPromptPanel.mount();", context);
  return { document };
}

test("mounts prompt panel without TrustedHTML / innerHTML on the host page", () => {
  const { document } = setup();
  const host = document.getElementById("sujiu-aistudio-host");
  assert.ok(host, "mounted host is present");
  assert.ok(host.shadowRoot, "Shadow DOM was created");
  const pane = host.shadowRoot.getElementById("pane");
  assert.ok(pane, "prompt pane exists");
  assert.equal(pane.hidden, false, "first install opens the panel");
  assert.ok(host.shadowRoot.getElementById("toggle"), "floating toggle exists");
  assert.equal(host.shadowRoot.querySelector('[data-role="system-prompt"]'), null, "system prompt editor was removed");
  assert.equal(host.shadowRoot.querySelector('[data-role="auto-inject"]'), null, "automatic system instruction injection was removed");
  assert.equal(host.shadowRoot.querySelector('[data-role="preset"]'), null, "preset selector was removed");
  assert.ok(host.shadowRoot.querySelector('[data-role="reduce-visual-effects"]'), "performance toggle remains");
  assert.ok(host.shadowRoot.querySelector('[data-role="keep"]'), "recent-round setting remains");
  assert.ok(host.shadowRoot.querySelector('[data-role="compress"]') === null);
  assert.ok(host.shadowRoot.querySelector('[data-role="summary-prompt"]'), "compression prompt input exists");
  assert.ok(host.shadowRoot.querySelector('[data-role="status"]'), "status node exists");
  assert.doesNotMatch(source, /\.innerHTML\s*=/);
});
