const PromptPanel = (() => {
    const STORAGE_V2 = "sujiu:aistudio:prompt-presets:v2";
    const STORAGE_V1 = "sujiu:aistudio:prompt-presets:v1";
    function load() {
      try {
        const raw = localStorage.getItem(STORAGE_V2) || localStorage.getItem(STORAGE_V1);
        return PromptProfiles.normalize(raw ? JSON.parse(raw) : null);
      } catch {
        return PromptProfiles.normalize(null);
      }
    }
    let data = load();
    let root = null;
    let panel = null;
    let lastAutoApplied = "";
    function save() {
      try {
        localStorage.setItem(STORAGE_V2, JSON.stringify(data));
      } catch (error) {
        status("预设保存失败：" + String(error.message || error));
      }
    }
    function current() {
      if (!Object.hasOwn(data.presets, data.active)) data = PromptProfiles.normalize(data);
      return data.presets[data.active];
    }
    const combined = () => PromptProfiles.compile(current());
    function status(message) {
      const el = root?.querySelector('[data-role="status"]');
      if (el) el.textContent = String(message);
    }
    function nativeField() {
      const selectors = [
        'ms-system-instructions textarea',
        'ms-system-instruction-editor textarea',
        'textarea[aria-label*="System instructions"]',
        'textarea[aria-label*="System Instructions"]',
        'textarea[aria-label*="系统指令"]',
        '[aria-label="System instructions"] [contenteditable="true"]',
        'ms-system-instructions [contenteditable="true"]'
      ];
      for (const doc of UI.getCandidateDocuments()) {
        for (const selector of selectors) {
          const el = doc.querySelector(selector);
          if (el && !el.disabled && !el.readOnly) return el;
        }
      }
      return null;
    }
    function applyNative(text) {
      const el = nativeField();
      if (!el || !text) return false;
      if (el.tagName === "TEXTAREA") {
        if (el.value === text) return true;
        const proto = el.ownerDocument.defaultView?.HTMLTextAreaElement?.prototype || HTMLTextAreaElement.prototype;
        const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
        if (!setter) return false;
        setter.call(el, text);
      } else if (el.isContentEditable) {
        if (el.textContent === text) return true;
        el.textContent = text;
      } else return false;
      el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: text }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
      return true;
    }
    function autoInjectOnce() {
      if (!userSettings.autoInject || state.compressionInProgress || state.branchInProgress) return;
      const text = combined();
      if (!text) return;
      const token = location.pathname + ":" + data.active + ":" + text;
      if (lastAutoApplied === token) return;
      if (applyNative(text)) {
        lastAutoApplied = token;
        status("已尝试同步系统指令；请确认 AI Studio 已保存");
      }
    }
    async function clipboard(text) {
      try {
        await navigator.clipboard.writeText(text);
        return true;
      } catch { return false; }
    }
    function renderFields() {
      if (!root) return;
      const profile = current();
      const container = root.getElementById("fields");
      for (const id of profile.order) {
        const row = root.querySelector('[data-section="' + id + '"]');
        row.querySelector('[data-enable="' + id + '"]').checked = profile.enabled[id] !== false;
        row.querySelector('[data-field="' + id + '"]').value = profile.fields[id] || "";
        container.appendChild(row);
      }
    }
    function render() {
      if (!root) return;
      const select = root.querySelector('[data-role="preset"]');
      select.replaceChildren();
      for (const name of Object.keys(data.presets)) {
        const option = document.createElement("option");
        option.value = name;
        option.textContent = name;
        select.appendChild(option);
      }
      select.value = data.active;
      renderFields();
      root.querySelector('[data-role="keep"]').value = String(userSettings.keepRounds || 8);
      root.querySelector('[data-role="threshold"]').value = String(userSettings.autoThreshold || 50);
      root.querySelector('[data-role="auto-compact"]').checked = Boolean(userSettings.autoCompact);
      root.querySelector('[data-role="auto-inject"]').checked = Boolean(userSettings.autoInject);
      root.querySelector('[data-role="summary-prompt"]').value = getCompressPrompt();
    }
    function exportPresets() {
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      try {
        const link = document.createElement("a");
        link.href = url;
        link.download = "sujiu-aistudio-presets.json";
        document.body.appendChild(link);
        link.click();
        link.remove();
        status("预设已导出");
      } finally {
        setTimeout(() => URL.revokeObjectURL(url), 5000);
      }
    }
    async function importPresets(file) {
      if (!file || file.size > 1024 * 1024) {
        status("导入失败：文件超过 1 MB 或未选择文件");
        return;
      }
      try {
        const raw = JSON.parse(await file.text());
        if (!raw || !raw.presets || typeof raw.presets !== "object" || Array.isArray(raw.presets) ||
            Object.keys(raw.presets).length === 0) throw new Error("预设结构不正确");
        if (!confirm("导入将替换现有预设。建议先导出备份，是否继续？")) return;
        data = PromptProfiles.normalize(raw);
        save();
        lastAutoApplied = "";
        render();
        status("已导入 " + Object.keys(data.presets).length + " 个预设");
      } catch (error) {
        status("预设导入失败：" + String(error?.message || error));
      }
    }
    function mount() {
      if (document.getElementById("sujiu-aistudio-host")) return;
      if (!document.body) {
        document.addEventListener("DOMContentLoaded", mount, { once: true });
        return;
      }
      const host = document.createElement("div");
      host.id = "sujiu-aistudio-host";
      host.style.cssText = "position:fixed;right:18px;bottom:18px;z-index:2147483000";
      const shadow = host.attachShadow({ mode: "open" });
      shadow.innerHTML = `
        <style>
          *{box-sizing:border-box} button,input,select,textarea{font:inherit}
          button{cursor:pointer;border:1px solid #64748b66;border-radius:8px;padding:6px 8px;background:#ffffff12;color:inherit}
          button:hover{background:#94a3b833}button:focus-visible,input:focus-visible,select:focus-visible,textarea:focus-visible{outline:2px solid #0d9488}
          #toggle{background:#156f67;color:white;border:0;box-shadow:0 3px 16px #0005}
          #pane{width:min(405px,calc(100vw - 24px));max-height:min(82vh,850px);margin-bottom:9px;overflow:auto;color:#f1f5f9;background:#1b2029;border:1px solid #64748b55;border-radius:14px;padding:15px;box-shadow:0 6px 40px #0008;font:13px/1.5 system-ui,sans-serif}
          #pane[hidden]{display:none}h2{font-size:15px;margin:0 0 10px}h3{font-size:13px;margin:16px 0 7px}.line{display:flex;align-items:center;gap:6px;margin:6px 0}.line>*{min-width:0}
          select,input[type=number],textarea{width:100%;background:#0f172a;color:#f8fafc;border:1px solid #64748b99;border-radius:8px;padding:7px}
          textarea{display:block;resize:vertical;min-height:62px;margin:5px 0 10px}
          label{font-size:12px;color:#cbd5e1}.section-header{display:flex;align-items:center;gap:7px;justify-content:space-between}
          .section-header label{flex:1}.small{padding:3px 7px;font-size:12px}
          #status{color:#99f6e4;min-height:1.5em;font-size:12px;margin-top:10px}
          .hint{color:#94a3b8;font-size:11px;margin:7px 0}
          .check-line{display:flex;gap:8px;align-items:center}.check-line input{width:auto}
          hr{border:0;border-top:1px solid #64748b55;margin:14px 0}
        </style>
        <section id="pane" hidden aria-label="素酒 AI Studio 工具">
          <h2>素酒 · AI Studio</h2>
          <label>提示词预设</label>
          <div class="line">
            <select data-role="preset" aria-label="当前预设"></select>
            <button data-action="new">新建</button><button data-action="delete">删除</button>
          </div>
          <div class="line"><button data-action="import">导入</button><button data-action="export">导出</button></div>
          <input data-role="file" type="file" accept=".json,application/json" hidden>
          <div id="fields"></div>
          <div class="line"><button data-action="apply">注入系统指令</button><button data-action="copy">复制组合内容</button></div>
          <label class="check-line"><input type="checkbox" data-role="auto-inject"> 自动同步提示词到系统指令框</label>
          <p class="hint">按勾选和顺序合并所有提示词；自动同步仅在 AI Studio 的原生编辑框可访问时执行。</p>
          <hr>
          <h3>上下文管理</h3>
          <label>保留最近完整轮数</label><input data-role="keep" type="number" min="1" max="100" step="1">
          <label class="check-line"><input type="checkbox" data-role="auto-compact"> 达到阈值后自动压缩旧上下文</label>
          <label>自动清理阈值（完整轮数）</label><input data-role="threshold" type="number" min="10" max="10000" step="1">
          <label>压缩摘要提示词</label><textarea data-role="summary-prompt" rows="5"></textarea>
          <div class="line"><button data-action="compress">立即压缩并创建分支</button></div>
          <p class="hint">清理会在新分支中用摘要替换较早轮次；原始聊天仍保留以便恢复。自动压缩默认关闭。</p>
          <div id="status" data-role="status" role="status" aria-live="polite"></div>
        </section>
        <button id="toggle" aria-expanded="false" aria-controls="pane">素酒工具</button>`;
      root = shadow;
      panel = shadow.getElementById("pane");
      const container = shadow.getElementById("fields");
      for (const section of PromptProfiles.sections) {
        const row = document.createElement("div");
        row.dataset.section = section.id;
        const head = document.createElement("div");
        head.className = "section-header";
        const label = document.createElement("label");
        const enable = document.createElement("input");
        enable.type = "checkbox";
        enable.dataset.enable = section.id;
        label.append(enable, document.createTextNode(" " + section.title));
        const up = document.createElement("button");
        up.textContent = "↑";
        up.className = "small";
        up.title = "上移" + section.title;
        up.dataset.move = section.id;
        up.dataset.delta = "-1";
        const down = document.createElement("button");
        down.textContent = "↓";
        down.className = "small";
        down.title = "下移" + section.title;
        down.dataset.move = section.id;
        down.dataset.delta = "1";
        head.append(label, up, down);
        const editor = document.createElement("textarea");
        editor.dataset.field = section.id;
        editor.rows = 3;
        editor.setAttribute("aria-label", section.title);
        row.append(head, editor);
        container.appendChild(row);
      }
      host.addEventListener("click", () => {}, { once: true });
      shadow.getElementById("toggle").addEventListener("click", () => {
        panel.hidden = !panel.hidden;
        shadow.getElementById("toggle").setAttribute("aria-expanded", String(!panel.hidden));
        if (!panel.hidden) render();
      });
      shadow.querySelector('[data-role="preset"]').addEventListener("change", event => {
        data.active = event.target.value;
        save(); lastAutoApplied = ""; render();
      });
      shadow.querySelector('[data-role="file"]').addEventListener("change", event => {
        void importPresets(event.target.files?.[0]);
        event.target.value = "";
      });
      shadow.addEventListener("input", event => {
        const id = event.target?.dataset?.field;
        if (id) {
          current().fields[id] = event.target.value.slice(0, 100000);
          save(); lastAutoApplied = "";
        }
      });
      shadow.addEventListener("change", event => {
        const el = event.target;
        const id = el?.dataset?.enable;
        if (id) {
          current().enabled[id] = el.checked;
          save(); lastAutoApplied = "";
          return;
        }
        const role = el?.dataset?.role;
        if (role === "auto-inject") {
          updateUserSettings({ autoInject: el.checked });
          lastAutoApplied = "";
          autoInjectOnce();
        }
        if (role === "auto-compact") {
          updateUserSettings({ autoCompact: el.checked });
          status(el.checked ? "自动压缩已开启，阈值只在完成回复后检查" : "自动压缩已关闭");
        }
        if (role === "keep" || role === "threshold") {
          const keep = Number(root.querySelector('[data-role="keep"]').value);
          const threshold = Number(root.querySelector('[data-role="threshold"]').value);
          if (!Number.isSafeInteger(keep) || keep < 1 || keep > 100 ||
              !Number.isSafeInteger(threshold) || threshold < 10 || threshold > 10000 || threshold <= keep) {
            status("轮数无效：阈值需要大于保留轮数，且保留至少 1 轮");
            render();
          } else {
            updateUserSettings({ keepRounds: keep, autoThreshold: threshold });
            status("保留最近 " + keep + " 轮，自动阈值 " + threshold + " 轮");
          }
        }
        if (role === "summary-prompt") {
          const value = el.value.trim();
          if (value) {
            updateUserSettings({ compressPrompt: value });
            refreshActivePromptFromSettings();
            status("压缩摘要指令已保存");
          }
        }
      });
      shadow.addEventListener("click", async event => {
        const moveId = event.target?.dataset?.move;
        if (moveId) {
          data.presets[data.active] = PromptProfiles.shift(current(), moveId, Number(event.target.dataset.delta));
          save(); lastAutoApplied = ""; renderFields();
          return;
        }
        const action = event.target?.dataset?.action;
        if (!action) return;
        if (action === "new") {
          const name = prompt("新预设名称");
          if (!name?.trim()) return;
          const cleaned = name.trim().slice(0, 80);
          if (["__proto__", "prototype", "constructor"].includes(cleaned) ||
              Object.hasOwn(data.presets, cleaned) || Object.keys(data.presets).length >= PromptProfiles.maxPresets) {
            status("预设名称不可用、已存在，或已达到 50 个上限");
            return;
          }
          data.presets[cleaned] = PromptProfiles.normalizedPreset(current());
          data.active = cleaned;
          save(); lastAutoApplied = ""; render(); status("已创建预设");
        }
        if (action === "delete") {
          if (Object.keys(data.presets).length <= 1) { status("至少保留一个预设"); return; }
          if (!confirm("删除预设「" + data.active + "」？")) return;
          delete data.presets[data.active];
          data.active = Object.keys(data.presets)[0];
          save(); lastAutoApplied = ""; render(); status("已删除预设");
        }
        if (action === "export") exportPresets();
        if (action === "import") root.querySelector('[data-role="file"]').click();
        if (action === "copy" || action === "apply") {
          const text = combined();
          if (!text) { status("至少需要一个已启用且有内容的提示词框"); return; }
          if (action === "apply" && applyNative(text)) {
            lastAutoApplied = location.pathname + ":" + data.active + ":" + text;
            status("已尝试填入系统指令，请确认 AI Studio 是否保存");
            return;
          }
          const ok = await clipboard(text);
          status(ok ? "已复制提示词，可粘贴至系统指令框" : "复制失败：请检查剪贴板权限");
        }
        if (action === "compress") {
          status("准备生成摘要并创建新分支，原会话仍会保留");
          Compression.run();
        }
      });
      document.body.appendChild(host);
      render();
      setInterval(autoInjectOnce, 1800);
    }
    return { mount, status };
  })();
