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
    this.listeners = Object.create(null);
    this.parentNode = null;
  }
  set innerHTML(_value) { throw new TypeError("This document requires TrustedHTML assignment"); }
  setAttribute(name, value) {
    this.attributes[name] = String(value);
    if (name === "id") this.id = String(value);
    if (name.startsWith("data-")) this.dataset[name.slice(5).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = String(value);
  }
  getAttribute(name) { return this.attributes[name] ?? null; }
  append(...children) {
    for (const child of children) {
      this.children.push(child);
      if (child && typeof child === "object") child.parentNode = this;
    }
  }
  appendChild(node) { this.append(node); return node; }
  replaceChildren(...children) { this.children = []; this.append(...children); }
  addEventListener(name, handler) { (this.listeners[name] ||= []).push(handler); }
  emit(name, event = {}) {
    for (const handler of this.listeners[name] || [])
      handler({ button: 0, pointerId: 1, clientX: 0, clientY: 0,
                target: this, preventDefault() {}, ...event });
  }
  getBoundingClientRect() {
    if (this.id === "toggle") {
      const host = this.parentNode?.host;
      const left = Number.parseFloat(host?.style.left) || 0;
      const top = Number.parseFloat(host?.style.top) || 0;
      return { left, top, width: 88, height: 36, right: left + 88, bottom: top + 36 };
    }
    if (this.id === "pane") {
      const w = Math.min(405, this.parentNode?.host?.viewport?.width - 24 || 405);
      const h = Math.min(520, (this.parentNode?.host?.viewport?.height || 768) * 0.82);
      const left = Number.parseFloat(this.style.left) || 0;
      const top = Number.parseFloat(this.style.top) || 0;
      return { left, top, width: w, height: h, right: left + w, bottom: top + h };
    }
    return { left: 0, top: 0, width: 0, height: 0, right: 0, bottom: 0 };
  }
  attachShadow() {
    this.shadowRoot = new StrictNode("shadow");
    this.shadowRoot.host = this;
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

function setup({ width = 1024, height = 768, store = new Map() } = {}) {
  const window = {
    innerWidth: width, innerHeight: height,
    listeners: Object.create(null),
    addEventListener(name, handler) { (this.listeners[name] ||= []).push(handler); },
    emit(name) { for (const handler of this.listeners[name] || []) handler(); }
  };
  const document = {
    body: new StrictNode("body"),
    listeners: Object.create(null),
    addEventListener(name, handler) { (this.listeners[name] ||= []).push(handler); },
    emit(name, event = {}) {
      for (const handler of this.listeners[name] || []) handler(event);
    },
    readyState: "complete",
    createElement: tag => new StrictNode(tag),
    createTextNode: text => ({ textContent: String(text) }),
    getElementById(id) { return this.body.getElementById(id); }
  };
  const context = {
    document,
    window,
    localStorage: {
      getItem: key => store.get(key) ?? null,
      setItem: (key, value) => store.set(key, String(value))
    },
    state: { compressionInProgress: false, branchInProgress: false },
    userSettings: { keepRounds: 8 },
    PerformanceTuning: { enabled: true, setEnabled(value) { this.enabled = Boolean(value); return this.enabled; } },
    getCompressPrompt: () => "Summary",
    setInterval: () => 1,
    console: { info: () => {} }
  };
  vm.runInNewContext(source + "\nPromptPanel.mount();", context);
  const host = document.getElementById("sujiu-aistudio-host");
  host.viewport = { width, height };
  return { document, host, window, store };
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


test("plugin never injects or manages Google System Instructions", () => {
  assert.doesNotMatch(source, /ms-system-instructions|ms-system-instruction-editor/i);
  assert.doesNotMatch(source, /autoInject|auto-inject|applyNative|PromptProfiles|prompt-presets|system-prompt/);
  assert.doesNotMatch(source, /[.]clipboard[.]writeText|data-action", "apply"/);
  assert.match(source, /role\("summary-prompt", "textarea"/);
  assert.match(source, /action\("直接清理旧历史", "cleanup-history"\)/);
  assert.match(source, /role\("reduce-visual-effects", "input"/);
});


test("mobile launcher defaults near the top, away from the send button", () => {
  const { host } = setup({ width: 390, height: 844 });
  const toggle = host.shadowRoot.getElementById("toggle");
  const rect = toggle.getBoundingClientRect();
  assert.ok(rect.top <= 120, "launcher is near the top of the mobile viewport");
  assert.ok(rect.right <= 390 && rect.left >= 0, "launcher stays inside the viewport");
  assert.equal(host.shadowRoot.getElementById("pane").hidden, true, "new mobile installs start collapsed");
  assert.equal(toggle.style.touchAction, "none", "drag handles touch without scrolling the page");
});

test("dragging launcher persists its position, suppresses click and survives reload", () => {
  const store = new Map();
  const first = setup({ width: 390, height: 844, store });
  const toggle = first.host.shadowRoot.getElementById("toggle");
  const initial = toggle.getBoundingClientRect();
  const targetX = initial.left - 230, targetY = initial.top + 170;
  toggle.emit("pointerdown", { clientX: initial.left + 20, clientY: initial.top + 12 });
  toggle.emit("pointermove", { clientX: initial.left + 20 - 230, clientY: initial.top + 12 + 170 });
  toggle.emit("pointerup", { clientX: initial.left + 20 - 230, clientY: initial.top + 12 + 170 });
  const rect = toggle.getBoundingClientRect();
  assert.ok(Math.abs(rect.left - targetX) < 1 && Math.abs(rect.top - targetY) < 1);
  assert.ok(store.has("sujiu:aistudio:launcher-position:v1"), "persisted location exists");
  toggle.emit("click");
  assert.equal(first.host.shadowRoot.getElementById("pane").hidden, true, "drag does not open panel");

  const second = setup({ width: 390, height: 844, store });
  const restored = second.host.shadowRoot.getElementById("toggle").getBoundingClientRect();
  assert.ok(Math.abs(restored.left - rect.left) < 1);
  assert.ok(Math.abs(restored.top - rect.top) < 1);
});

test("short taps open the panel and its overlay remains in view", () => {
  const { host } = setup({ width: 390, height: 844 });
  const toggle = host.shadowRoot.getElementById("toggle");
  const pane = host.shadowRoot.getElementById("pane");
  toggle.emit("pointerdown", { clientX: 340, clientY: 90 });
  toggle.emit("pointerup", { clientX: 340, clientY: 90 });
  toggle.emit("click");
  assert.equal(pane.hidden, false);
  assert.equal(toggle.getAttribute("aria-expanded"), "true");
  const rect = pane.getBoundingClientRect();
  assert.ok(rect.left >= 12 && rect.right <= 390 - 12);
  assert.ok(rect.top >= 12 && rect.bottom <= 844 - 12);
  toggle.emit("click");
  assert.equal(pane.hidden, true);
});

test("saved launcher position is clamped when the viewport shrinks", () => {
  const store = new Map([["sujiu:aistudio:launcher-position:v1", JSON.stringify({ x: 1, y: 1 })]]);
  const { host, window } = setup({ width: 390, height: 844, store });
  const toggle = host.shadowRoot.getElementById("toggle");
  window.innerWidth = 320; window.innerHeight = 480;
  window.emit("resize");
  const rect = toggle.getBoundingClientRect();
  assert.ok(rect.left >= 12 && rect.right <= 320 - 12);
  assert.ok(rect.top >= 12 && rect.bottom <= 480 - 12);
});


test("mobile panel has a close action that remains available even if launcher is covered", () => {
  const { host } = setup({ width: 390, height: 844 });
  const shadow = host.shadowRoot;
  const toggle = shadow.getElementById("toggle");
  const pane = shadow.getElementById("pane");
  toggle.emit("click");
  assert.equal(pane.hidden, false, "panel opened");
  const close = shadow.querySelector('[data-action="close-panel"]');
  assert.ok(close, "panel header contains a close button");
  shadow.emit("click", { target: close });
  assert.equal(pane.hidden, true, "close button collapses panel");
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
  assert.equal(toggle.style.position, "relative", "launcher is in an explicit stacking layer");
  assert.ok(Number(toggle.style.zIndex) > Number(pane.style.zIndex),
    "launcher remains visually above the panel");
});

test("tapping outside the panel dismisses it; tapping inside does not", () => {
  const { host, document } = setup({ width: 390, height: 844 });
  const shadow = host.shadowRoot;
  const toggle = shadow.getElementById("toggle");
  const pane = shadow.getElementById("pane");
  toggle.emit("click");
  document.emit("pointerdown", {
    target: pane,
    composedPath: () => [pane, shadow, host, document.body, document]
  });
  assert.equal(pane.hidden, false, "tap inside doesn't dismiss");
  document.emit("pointerdown", {
    target: document.body,
    composedPath: () => [document.body, document]
  });
  assert.equal(pane.hidden, true, "tap outside dismisses panel");
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
});

test("Escape key dismisses panel even when its content has focus", () => {
  const { host, document } = setup({ width: 390, height: 844 });
  const toggle = host.shadowRoot.getElementById("toggle");
  const pane = host.shadowRoot.getElementById("pane");
  toggle.emit("click");
  document.emit("keydown", { key: "Escape" });
  assert.equal(pane.hidden, true);
});

test("the mobile panel is height-limited and has a sticky close header", () => {
  const { host } = setup({ width: 390, height: 600 });
  const pane = host.shadowRoot.getElementById("pane");
  const toggle = host.shadowRoot.getElementById("toggle");
  toggle.emit("click");
  assert.match(pane.style.maxHeight, /72dvh/);
  assert.match(source, /\.panel-header\{position:sticky;/);
  assert.match(source, /#pane\[hidden\]\{display:none\}/);
  assert.ok(pane.style.overflow === "auto");
});
