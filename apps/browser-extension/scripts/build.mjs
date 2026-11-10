const fs = await import("node:fs/promises");
  const path = await import("node:path");
  const root = path.resolve(import.meta.dirname || path.dirname(new URL(import.meta.url).pathname), "..");
  const files = ["00-state.js","10-settings.js","25-compaction-task.js","28-request-scope.js","20-network.js","30-compression.js","35-rounds.js","40-history.js","50-branching.js","55-in-place.js","60-native-ui.js","65-performance.js","67-save-monitor.js","70-utils.js","80-prompt-panel.js","90-bootstrap.js"];
  const text = (await Promise.all(files.map(file => fs.readFile(path.join(root,"src",file),"utf8")))).join("\n");
  const bundle = [
    '(function () {',
    '"use strict";',
    'const bootstrapStatus = { version: "0.4.10", phase: "loaded", loadedAt: new Date().toISOString() };',
    'window.__SUJIU_AISTUDIO_BOOT__ = bootstrapStatus;',
    'console.info("[Sujiu AI Studio] injected", bootstrapStatus.version, location.href);',
    'try {',
    'const unsafeWindow = window;',
    text,
    'bootstrapStatus.phase = "ready";',
    '} catch (error) {',
    '  bootstrapStatus.phase = "error";',
    '  bootstrapStatus.error = String(error?.message || error);',
    '  console.error("[Sujiu AI Studio] startup failure", error);',
    '  const renderFailure = () => {',
    '    if (!document.body || document.getElementById("sujiu-startup-error")) return;',
    '    const notice = document.createElement("button");',
    '    notice.id = "sujiu-startup-error";',
    '    notice.type = "button";',
    '    notice.textContent = "素酒启动失败 · 点击查看原因";',
    '    notice.title = bootstrapStatus.error;',
    '    notice.style.cssText = "position:fixed;right:16px;bottom:16px;z-index:2147483647;padding:12px;border:0;border-radius:10px;background:#a52a2a;color:white;cursor:pointer";',
    '    notice.addEventListener("click", () => alert("素酒启动失败：" + bootstrapStatus.error));',
    '    document.body.appendChild(notice);',
    '  };',
    '  if (document.body) renderFailure();',
    '  else document.addEventListener("DOMContentLoaded", renderFailure, { once: true });',
    '}',
    '})();',
    ''
  ].join("\n");
  await fs.mkdir(path.join(root,"dist","extension"), {recursive:true});
  await fs.writeFile(path.join(root,"dist","extension","main.js"),bundle);
  const manifest = {
    manifest_version:3,
    name:"素酒 · AI Studio 工具",
    version:"0.4.10",
    description:"AI Studio 全历史摘要、手动清理与性能优化",
    content_scripts:[{matches:["https://aistudio.google.com/*"],js:["main.js"],run_at:"document_start",world:"MAIN"}]
  };
  await fs.writeFile(path.join(root,"dist","extension","manifest.json"),JSON.stringify(manifest,null,2)+"\n");
  const header = [
    "// ==UserScript==",
    "// @name         Sujiu AI Studio Tools",
    "// @namespace    https://github.com/Xiwei753/sujiu",
    "// @version      0.4.10",
    "// @updateURL    https://raw.githubusercontent.com/Xiwei753/sujiu/main/apps/browser-extension/sujiu-aistudio.user.js",
    "// @downloadURL  https://raw.githubusercontent.com/Xiwei753/sujiu/main/apps/browser-extension/sujiu-aistudio.user.js",
    "// @description  AI Studio manual context compaction and history cleanup",
    "// @match        https://aistudio.google.com/*",
    "// @noframes",
    "// @sandbox      raw",
    "// @run-at       document-start",
    "// @grant        none",
    "// ==/UserScript==",
    ""
  ].join("\n");
  await fs.writeFile(path.join(root,"dist","sujiu-aistudio.user.js"),header+bundle);
  await fs.writeFile(path.join(root,"sujiu-aistudio.user.js"),header+bundle);
  process.stdout.write("Generated userscript and MV3 extension.\n");
