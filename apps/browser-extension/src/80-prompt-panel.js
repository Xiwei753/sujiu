const PromptPanel = (() => {
    const key = "sujiu:aistudio:prompt-presets:v1";
    const sections = [
      ["system", "系统提示词"],
      ["character", "角色设定"],
      ["world", "世界设定"],
      ["persona", "用户人物卡"],
      ["author", "作者附注"]
    ];
    const empty = () => Object.fromEntries(sections.map(([id]) => [id, ""]));
    const load = () => {
      try {
        const saved = JSON.parse(localStorage.getItem(key) || "null");
        if (saved && typeof saved === "object" && saved.presets && typeof saved.presets === "object") {
          return {
            active: typeof saved.active === "string" ? saved.active : "默认",
            presets: saved.presets
          };
        }
      } catch { /* recover from damaged local settings */ }
      return { active: "默认", presets: { "默认": empty() } };
    };
    const data = load();
    let root = null;
    let panel = null;
    const persist = () => localStorage.setItem(key, JSON.stringify(data));
    const current = () => data.presets[data.active] || (data.presets[data.active] = empty());
    const combined = () => sections
      .map(([id, title]) => String(current()[id] || "").trim() ? "[" + title + "]\n" + String(current()[id]).trim() : "")
      .filter(Boolean).join("\n\n");
    function setStatus(message) {
      const target = root?.querySelector('[data-role="status"]');
      if (target) target.textContent = message;
    }
    function applyNativeInstructions(text) {
      const selectors = [
        'ms-system-instructions textarea',
        'ms-system-instruction-editor textarea',
        'textarea[aria-label*="System instructions"]',
        'textarea[aria-label*="System Instructions"]',
        '[aria-label="System instructions"] [contenteditable="true"]',
        'ms-system-instructions [contenteditable="true"]'
      ];
      for (const doc of UI.getCandidateDocuments()) {
        for (const selector of selectors) {
          const element = doc.querySelector(selector);
          if (!element || element.disabled || element.readOnly) continue;
          if (element instanceof HTMLTextAreaElement) {
            const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
            if (!setter) continue;
            setter.call(element, text);
          } else if (element.isContentEditable) {
            element.textContent = text;
          } else continue;
          element.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: text }));
          element.dispatchEvent(new Event("change", { bubbles: true }));
          return true;
        }
      }
      return false;
    }
    async function copy(text) {
      try { await navigator.clipboard.writeText(text); return true; }
      catch { return false; }
    }
    function renderValues() {
      if (!root) return;
      const select = root.querySelector('[data-role="preset"]');
      select.replaceChildren();
      for (const name of Object.keys(data.presets)) {
        const opt = document.createElement("option");
        opt.value = name;
        opt.textContent = name;
        select.appendChild(opt);
      }
      if (!data.presets[data.active]) data.active = Object.keys(data.presets)[0] || "默认";
      select.value = data.active;
      for (const [id] of sections) {
        root.querySelector('[data-field="' + id + '"]').value = current()[id] || "";
      }
      root.querySelector('[data-role="keep"]').value = String(Number(userSettings.keepRounds) || 8);
      root.querySelector('[data-role="summary-prompt"]').value = getCompressPrompt();
    }
    function createElement() {
      if (document.getElementById("sujiu-aistudio-host")) return;
      const host = document.createElement("div");
      host.id = "sujiu-aistudio-host";
      host.style.cssText = "position:fixed;right:18px;bottom:18px;z-index:2147483000";
      const shadow = host.attachShadow({ mode: "open" });
      shadow.innerHTML = `
        <style>
          *{box-sizing:border-box}button,input,select,textarea{font:inherit}
          button{cursor:pointer;border:1px solid #6665;border-radius:9px;padding:7px 10px;background:#ffffff12;color:inherit}
          button:hover{background:#8883}button:focus-visible,textarea:focus-visible,input:focus-visible,select:focus-visible{outline:2px solid #29b6b2}
          #toggle{background:#166c67;color:white;border:0;box-shadow:0 3px 16px #0005}
          #pane{width:min(390px,calc(100vw - 24px));max-height:min(83vh,850px);margin-bottom:9px;overflow:auto;color:#f1f5f9;background:#1b2029;border:1px solid #64748b55;border-radius:14px;padding:14px;box-shadow:0 6px 40px #0008;font:13px/1.5 system-ui,sans-serif}
          #pane[hidden]{display:none}h2{font-size:15px;margin:0 0 12px}.line{display:flex;align-items:center;gap:8px;margin:8px 0}.line>*{min-width:0}
          select,input,textarea{width:100%;background:#0f172a;color:#f8fafc;border:1px solid #64748b99;border-radius:8px;padding:7px}
          textarea{display:block;resize:vertical;min-height:66px;margin:4px 0 9px}
          label{display:block;font-size:12px;color:#cbd5e1}#actions{display:flex;flex-wrap:wrap;gap:7px}
          #status{color:#99f6e4;min-height:1.5em;font-size:12px;margin-top:10px}
          .hint{color:#94a3b8;font-size:11px;margin:5px 0}
        </style>
        <section id="pane" hidden aria-label="素酒 AI Studio 工具">
          <h2>素酒 · AI Studio</h2>
          <label>提示词预设</label>
          <div class="line"><select data-role="preset" aria-label="预设"></select><button data-action="new">新建</button><button data-action="delete">删除</button></div>
          <div id="fields"></div>
          <div id="actions"><button data-action="apply">填入系统指令</button><button data-action="copy">复制组合提示词</button></div>
          <hr style="border:0;border-top:1px solid #64748b55;margin:14px 0">
          <label>压缩后保留完整轮数（默认 8）</label>
          <input data-role="keep" type="number" min="1" max="100" step="1">
          <label>压缩摘要指令</label>
          <textarea data-role="summary-prompt" rows="5"></textarea>
          <div id="actions"><button data-action="compress">手动压缩并创建分支</button></div>
          <p class="hint">先保存原聊天；压缩通过 Google AI Studio 内部接口创建新分支。接口可能随网站更新失效。</p>
          <div id="status" data-role="status" role="status" aria-live="polite"></div>
        </section>
        <button id="toggle" aria-expanded="false" aria-controls="pane">素酒工具</button>
      `;
      root = shadow;
      panel = shadow.getElementById("pane");
      const fields = shadow.getElementById("fields");
      for (const [id, title] of sections) {
        const label = document.createElement("label");
        label.textContent = title;
        const editor = document.createElement("textarea");
        editor.dataset.field = id;
        editor.rows = 3;
        editor.setAttribute("aria-label", title);
        editor.addEventListener("input", () => {
          current()[id] = editor.value;
          persist();
        });
        fields.append(label, editor);
      }
      shadow.getElementById("toggle").addEventListener("click", () => {
        panel.hidden = !panel.hidden;
        shadow.getElementById("toggle").setAttribute("aria-expanded", String(!panel.hidden));
        if (!panel.hidden) renderValues();
      });
      shadow.querySelector('[data-role="preset"]').addEventListener("change", event => {
        data.active = event.target.value;
        persist();
        renderValues();
      });
      shadow.querySelector('[data-role="keep"]').addEventListener("change", event => {
        const n = Number(event.target.value);
        if (Number.isSafeInteger(n) && n >= 1 && n <= 100) {
          updateUserSettings({ keepRounds: n });
          setStatus("已保存保留轮数：" + n);
        } else renderValues();
      });
      shadow.querySelector('[data-role="summary-prompt"]').addEventListener("change", event => {
        const value = event.target.value.trim();
        if (value) {
          updateUserSettings({ compressPrompt: value });
          refreshActivePromptFromSettings();
          setStatus("压缩指令已保存");
        }
      });
      shadow.addEventListener("click", async event => {
        const action = event.target?.dataset?.action;
        if (!action) return;
        if (action === "new") {
          const name = prompt("新预设名称");
          if (!name?.trim()) return;
          const cleaned = name.trim().slice(0, 80);
          if (data.presets[cleaned]) { setStatus("已存在同名预设"); return; }
          data.presets[cleaned] = { ...current() };
          data.active = cleaned;
          persist(); renderValues(); setStatus("已创建预设");
        }
        if (action === "delete") {
          if (Object.keys(data.presets).length === 1) { setStatus("至少保留一个预设"); return; }
          if (!confirm("删除预设「" + data.active + "」？")) return;
          delete data.presets[data.active];
          data.active = Object.keys(data.presets)[0];
          persist(); renderValues(); setStatus("已删除预设");
        }
        if (action === "copy" || action === "apply") {
          const text = combined();
          if (!text) { setStatus("先填写至少一项提示词"); return; }
          if (action === "apply" && applyNativeInstructions(text)) {
            setStatus("已填入系统指令，请检查 AI Studio 是否保存"); return;
          }
          const copied = await copy(text);
          setStatus(copied ? "已复制；可粘贴到系统指令" : "找不到可用的系统指令编辑框，且复制失败");
        }
        if (action === "compress") {
          setStatus("开始压缩：原会话会保留，成功后跳转新分支");
          Compression.run();
        }
      });
      document.body.appendChild(host);
      renderValues();
    }
    return {
      mount() {
        if (document.body) createElement();
        else document.addEventListener("DOMContentLoaded", createElement, { once: true });
      }
    };
  })();
