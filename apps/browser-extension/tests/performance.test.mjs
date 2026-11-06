import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import { readFile } from "node:fs/promises";

const source = await readFile(new URL("../src/65-performance.js", import.meta.url), "utf8");
function fixture(storedValue = null) {
  const nodes = new Map(), settings = new Map();
  if (storedValue !== null) settings.set("sujiu:aistudio:reduce-visual-effects:v1", storedValue);
  const document = {
    readyState: "complete",
    getElementById(id) { return nodes.get(id) || null; },
    createElement(tag) {
      const node = {
        tagName: tag, id: "", textContent: "",
        remove() { nodes.delete(this.id); }
      };
      return node;
    },
    head: {
      appendChild(node) { nodes.set(node.id, node); return node; }
    }
  };
  const context = {
    document,
    localStorage: {
      getItem(key) { return settings.get(key) ?? null; },
      setItem(key, val) { settings.set(key, val); }
    }
  };
  const performance = vm.runInNewContext(source + "\nPerformanceTuning", context);
  return { performance, nodes, settings, document };
}

test("reduces blur and smooth scrolling with one reversible style tag", () => {
  const { performance: p, nodes } = fixture();
  p.init();
  p.init();
  assert.equal(nodes.size, 1);
  const css = nodes.values().next().value.textContent;
  assert.match(css, /backdrop-filter:\s*none/);
  assert.match(css, /scroll-behavior:\s*auto/);
  assert.equal(p.setEnabled(false), false);
  assert.equal(nodes.size, 0);
  assert.equal(p.setEnabled(true), true);
  assert.equal(nodes.size, 1);
});
test("respects explicit opt-out and never touches chat nodes", () => {
  const { performance: p, nodes } = fixture("false");
  p.init();
  assert.equal(nodes.size, 0);
  assert.doesNotMatch(source, /ms-chat-turn|removeChild|innerHTML|display:\s*none/i);
});
