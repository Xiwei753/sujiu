import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import { readFile } from "node:fs/promises";

const source = await readFile(new URL("../src/68-native-save.js", import.meta.url), "utf8");
function control({ name, title, icon, text = "", disabled = false } = {}) {
  let clicks = 0;
  return {
    textContent: text, disabled,
    getAttribute(attr) {
      if (attr === "aria-label") return name || null;
      if (attr === "title") return title || null;
      return null;
    },
    querySelector() { return icon ? { textContent: icon } : null; },
    click() { clicks++; },
    get clicks() { return clicks; }
  };
}
function setup({ toolbarButtons = [], menuItems = [], moreLabel = null } = {}) {
  let menuOpen = false;
  const more = control({ name: moreLabel });
  const originalClick = more.click.bind(more);
  more.click = () => { originalClick(); menuOpen = true; };
  const toolbar = {
    querySelectorAll() { return moreLabel ? [...toolbarButtons, more] : toolbarButtons; }
  };
  const doc = {
    querySelector(sel) {
      return /ms-toolbar/.test(sel) ? toolbar : null;
    },
    querySelectorAll(sel) {
      return /cdk-overlay-container/.test(sel) && menuOpen ? menuItems : [];
    },
    dispatchEvent() {}
  };
  const native = vm.runInNewContext(source + "\nNativeSave", {
    document: doc, KeyboardEvent: class {}, setTimeout
  });
  return { native, more };
}

test("clicks a genuine native Save exactly once", async () => {
  const save = control({ name: "Save", icon: "save" });
  const x = setup({ toolbarButtons: [save] });
  assert.deepEqual(JSON.parse(JSON.stringify(await x.native.triggerOnce())),
    { triggered: true, via: "toolbar" });
  assert.equal(save.clicks, 1);
});

test("opens AI Studio overflow menu then saves once", async () => {
  const item = control({ text: "保存提示词" });
  const x = setup({ menuItems: [item], moreLabel: "More options" });
  const result = await x.native.triggerOnce();
  assert.equal(result.triggered, true);
  assert.equal(result.via, "menu");
  assert.equal(item.clicks, 1);
  assert.equal(x.more.clicks, 1);
});

test("never clicks Save As, export, or arbitrary icon", async () => {
  const saveAs = control({ name: "Save As", icon: "save" });
  const exportBtn = control({ name: "Export" });
  const x = setup({ toolbarButtons: [saveAs, exportBtn] });
  const result = await x.native.triggerOnce();
  assert.equal(result.triggered, false);
  assert.equal(saveAs.clicks, 0);
  assert.equal(exportBtn.clicks, 0);
});

test("no private save RPC or repeated server verification is synthesized", () => {
  assert.doesNotMatch(source, /\/$rpc\/|fetch\(|XMLHttpRequest|setInterval|ctrl\+s/i);
});
