const PromptPanel = (() => {
    const STORAGE_V3 = "sujiu:aistudio:prompt-presets:v3";
    const STORAGE_V2 = "sujiu:aistudio:prompt-presets:v2";
    const STORAGE_BACKUP = "sujiu:aistudio:prompt-presets:backup:v2";
    const STORAGE_V1 = "sujiu:aistudio:prompt-presets:v1";
    function load() {
      try {
        const current = localStorage.getItem(STORAGE_V3);
        if (current) return PromptProfiles.normalize(JSON.parse(current));
        const legacy = localStorage.getItem(STORAGE_V2) || localStorage.getItem(STORAGE_V1);
        const converted = PromptProfiles.normalize(legacy ? JSON.parse(legacy) : null);
        if (legacy) {
          // Make migration restart-safe, including all previously disabled
          // drafts. Keep the original v1/v2 storage untouched as a fallback.
          if (!localStorage.getItem(STORAGE_BACKUP))
            localStorage.setItem(STORAGE_BACKUP, legacy);
          localStorage.setItem(STORAGE_V3, JSON.stringify(converted));
        }
        return converted;
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
        localStorage.setItem(STORAGE_V3, JSON.stringify(data));
      } catch (error) {
        status("预设保存失败：" + String(error.message || error));
      }
    }
    function current() {
      if (!Object.hasOwn(data.presets, data.active)) data = PromptProfiles.normalize(data);
      return data.presets[data.active];
    }
    const combined = () => PromptProfiles.compile(current());
    function showCompactionReview(plan) {
      const container = root?.getElementById("inplace-review");
      if (!container) return;
      container.hidden = !plan;
      if (plan) {
        root.querySelector('[data-role="snapshot-preview"]').value = plan.snapshot;
        const warning = plan.legacy
          ? "这是旧版摘要，可能把过去剧情写成当前状态；建议重新生成后再清理。"
          : "请确认已结束事件、最新剧情时间线和摘要内容。";
        status("已读取摘要：" + plan.totalRounds + " 轮中，拟删除旧 " +
          plan.olderRounds + " 轮、保留最近 " + plan.recentRounds + " 轮。" + warning);
      }
    }
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
    function renderSystemPrompt() {
      if (!root) return;
      root.querySelector('[data-role="system-prompt"]').value = current().text || "";
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
      renderSystemPrompt();
      root.querySelector('[data-role="keep"]').value = String(userSettings.keepRounds || 8);
      root.querySelector('[data-role="auto-inject"]').checked = Boolean(userSettings.autoInject);
      root.querySelector('[data-role="reduce-visual-effects"]').checked = PerformanceTuning.enabled;
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
        status("已导入 " + Object.keys(data.presets).length + " 个预设，旧版多段内容已合并为系统提示词");
      } catch (error) {
        status("预设导入失败：" + String(error?.message || error));
      }
    }
    function mount() {
      if (document.getElementById("sujiu-aistudio-host")) return;
      // At document-start the body may not exist yet; wait for it rather
      // than trying to attach a fixed overlay to an incomplete DOM.
      if (!document.body) {
        if (document.readyState === "loading") {
          document.addEventListener("DOMContentLoaded", mount, { once: true });
        }
        if (!mount.retryScheduled) {
          mount.retryScheduled = true;
          const retry = setInterval(() => {
            if (!document.body) return;
            clearInterval(retry);
            mount.retryScheduled = false;
            mount();
          }, 250);
        }
        return;
      }
      const host = document.createElement("div");
      host.id = "sujiu-aistudio-host";
      host.style.cssText = "position:fixed;right:18px;bottom:18px;z-index:2147483000";
      const shadow = host.attachShadow({ mode: "open" });
      // AI Studio enforces Trusted Types; construct the panel with DOM APIs.
      // Do not assign strings to innerHTML / insertAdjacentHTML / document.write.
      const make = (tag, attributes = {}, children = []) => {
        const element = document.createElement(tag);
        for (const [name, value] of Object.entries(attributes)) {
          if (name === "text") element.textContent = String(value);
          else if (name === "className") element.className = String(value);
          else if (name === "hidden") element.hidden = Boolean(value);
          else element.setAttribute(name, String(value));
        }
        element.append(...children);
        return element;
      };
      const style = make("style", { text: "\n          *{box-sizing:border-box} button,input,select,textarea{font:inherit}\n          button{cursor:pointer;border:1px solid #64748b66;border-radius:8px;padding:6px 8px;background:#ffffff12;color:inherit}\n          button:hover{background:#94a3b833}button:focus-visible,input:focus-visible,select:focus-visible,textarea:focus-visible{outline:2px solid #0d9488}\n          #toggle{background:#156f67;color:white;border:0;box-shadow:0 3px 16px #0005}\n          #pane{width:min(405px,calc(100vw - 24px));max-height:min(82vh,850px);margin-bottom:9px;overflow:auto;color:#f1f5f9;background:#1b2029;border:1px solid #64748b55;border-radius:14px;padding:15px;box-shadow:0 6px 40px #0008;font:13px/1.5 system-ui,sans-serif}\n          #pane[hidden]{display:none}h2{font-size:15px;margin:0 0 10px}h3{font-size:13px;margin:16px 0 7px}.line{display:flex;align-items:center;gap:6px;margin:6px 0}.line>*{min-width:0}\n          select,input[type=number],textarea{width:100%;background:#0f172a;color:#f8fafc;border:1px solid #64748b99;border-radius:8px;padding:7px}\n          textarea{display:block;resize:vertical;min-height:62px;margin:5px 0 10px}\n          label{font-size:12px;color:#cbd5e1}\n          #status{color:#99f6e4;min-height:1.5em;font-size:12px;margin-top:10px}\n          .hint{color:#94a3b8;font-size:11px;margin:7px 0}\n          .check-line{display:flex;gap:8px;align-items:center}.check-line input{width:auto}\n          hr{border:0;border-top:1px solid #64748b55;margin:14px 0}\n          #inplace-review[hidden]{display:none}\n        " });
      const line = (...children) => make("div", { className: "line" }, children);
      const action = (label, name) => make("button", { type: "button", "data-action": name, text: label });
      const role = (name, tag = "input", attributes = {}) =>
        make(tag, { ...attributes, "data-role": name });
      const heading = make("h2", { text: "素酒 · AI Studio" });
      const pane = make("section", { id: "pane", hidden: true, "aria-label": "素酒 AI Studio 工具" }, [
        heading,
        make("label", { text: "提示词预设" }),
        line(role("preset", "select", { "aria-label": "当前预设" }), action("新建", "new"), action("删除", "delete")),
        line(action("导入", "import"), action("导出", "export")),
        role("file", "input", { type: "file", accept: ".json,application/json", hidden: true }),
        make("label", { text: "系统提示词" }),
        role("system-prompt", "textarea", { rows: "10", "aria-label": "系统提示词" }),
        line(action("注入系统指令", "apply"), action("复制系统提示词", "copy")),
        make("label", { className: "check-line" }, [
          role("auto-inject", "input", { type: "checkbox" }),
          document.createTextNode(" 自动同步到 AI Studio 系统指令")
        ]),
        make("p", { className: "hint", text: "只有这一段系统提示词会注入 AI Studio；不会反复添加到用户聊天记录。旧版多段预设会自动合并，禁用段落保留在预设导出数据中。" }),
        make("hr"),
        make("h3", { text: "性能" }),
        make("label", { className: "check-line" }, [
          role("reduce-visual-effects", "input", { type: "checkbox" }),
          document.createTextNode(" 减少页面特效（模糊与平滑滚动）")
        ]),
        make("p", { className: "hint", text: "可减少 GPU 重绘引起的卡顿；不会删减消息，也无法解决超长对话的全部 DOM 和 token 计算开销。" }),
        make("h3", { text: "上下文管理" }),
        make("label", { text: "保留最近完整轮数" }),
        role("keep", "input", { type: "number", min: "1", max: "100", step: "1" }),
        make("label", { text: "压缩摘要提示词" }),
        role("summary-prompt", "textarea", { rows: "5" }),
        line(action("生成摘要（当前聊天）", "compress")),
        make("p", { className: "hint", text: "生成没有固定等待时限。模型完成回复后，再点击读取摘要；两项操作相互独立。" }),
        line(action("读取已完成摘要", "recover-summary"), action("停止插件监听", "stop-tracking")),
        line(action("直接清理旧历史", "cleanup-history")),
        make("p", { className: "hint", text: "清理历史可以单独使用：按保留轮数删除较早的完整对话，已标记压缩记忆会保留。无需先生成或读取摘要；清理前会显示数量让你确认。" }),
        make("div", { id: "inplace-review", hidden: true }, [
          make("label", { text: "摘要预览（已写入原聊天）" }),
          role("snapshot-preview", "textarea", { rows: "8", readonly: "readonly" }),
          line(action("收起摘要预览", "cancel-delete"))
        ]),
        make("p", { className: "hint", text: "生成摘要不会自动删除历史；独立清理是不可撤销操作，使用 Google 原生删除菜单和自动保存。重要聊天请先备份。" }),
        make("div", { id: "status", "data-role": "status", role: "status", "aria-live": "polite" })
      ]);
      const toggle = make("button", { id: "toggle", type: "button",
        "aria-expanded": "false", "aria-controls": "pane", text: "素酒工具" });
      shadow.append(style, pane, toggle);
      // Inline style fallback if the host blocks injecting a <style> element.
      pane.style.width = "min(405px, calc(100vw - 24px))";
      pane.style.maxHeight = "min(82vh, 850px)";
      pane.style.overflow = "auto";
      pane.style.backgroundColor = "#1b2029";
      pane.style.color = "#f1f5f9";
      pane.style.padding = "15px";
      pane.style.borderRadius = "14px";
      toggle.style.backgroundColor = "#156f67";
      toggle.style.color = "white";
      toggle.style.padding = "8px 12px";
      toggle.style.borderRadius = "9px";
      toggle.style.cursor = "pointer";
      root = shadow;
      panel = shadow.getElementById("pane");
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
        if (event.target?.dataset?.role === "system-prompt") {
          current().text = event.target.value.slice(0, PromptProfiles.maxTextLength);
          save(); lastAutoApplied = "";
        }
      });
      shadow.addEventListener("change", event => {
        const el = event.target;
        const role = el?.dataset?.role;
        if (role === "reduce-visual-effects") {
          const enabled = PerformanceTuning.setEnabled(el.checked);
          status(enabled ? "已减少页面模糊与平滑滚动，可随时关闭" : "已恢复网页原有视觉效果");
        }
        if (role === "auto-inject") {
          updateUserSettings({ autoInject: el.checked });
          lastAutoApplied = "";
          autoInjectOnce();
        }
        if (role === "keep") {
          const keep = Number(el.value);
          if (!Number.isSafeInteger(keep) || keep < 1 || keep > 100) {
            status("保留轮数无效：请输入 1–100 之间的整数");
            render();
          } else {
            updateUserSettings({ keepRounds: keep });
            status("摘要请求和独立历史清理均保留最近 " + keep + " 轮完整对话");
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
          if (!text.trim()) { status("请先填写系统提示词"); return; }
          if (action === "apply" && applyNative(text)) {
            lastAutoApplied = location.pathname + ":" + data.active + ":" + text;
            status("已尝试填入系统指令，请确认 AI Studio 是否保存");
            return;
          }
          const ok = await clipboard(text);
          status(ok ? "已复制系统提示词，可粘贴至 AI Studio 系统指令框" : "复制失败：请检查剪贴板权限");
        }
        if (action === "compress") {
          status("已开始生成摘要。模型完成后请点击「读取已完成摘要」；不会自动删除任何历史消息");
          Compression.run();
        }
        if (action === "recover-summary") await Compression.recover();
        if (action === "stop-tracking") Compression.stopTracking();
        if (action === "cleanup-history") {
          try { await InPlaceCompaction.manualCleanup(); }
          catch (error) {
            const reason = String(error?.message || error);
            status("删除未完成：" + reason);
            console.warn("[Sujiu AI Studio] in-place deletion halted", reason);
          }
        }
        if (action === "cancel-delete") InPlaceCompaction.cancel();
      });
      // Open once after a fresh install so users can confirm injection
      // without having to find the small bottom-right launcher.
      try {
        const seenKey = "sujiu:aistudio:panel-opened:v1";
        if (!localStorage.getItem(seenKey)) {
          panel.hidden = false;
          shadow.getElementById("toggle").setAttribute("aria-expanded", "true");
          localStorage.setItem(seenKey, "1");
        }
      } catch {
        panel.hidden = false;
        shadow.getElementById("toggle").setAttribute("aria-expanded", "true");
      }
      document.body.appendChild(host);
      console.info("[Sujiu AI Studio] prompt panel mounted");
      render();
      setInterval(autoInjectOnce, 1800);
    }
    return { mount, status, showCompactionReview };
  })();
