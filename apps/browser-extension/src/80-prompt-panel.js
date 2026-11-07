const PromptPanel = (() => {
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
      shadow.addEventListener("change", event => {
        const el = event.target;
        const role = el?.dataset?.role;
        if (role === "reduce-visual-effects") {
          const enabled = PerformanceTuning.setEnabled(el.checked);
          status(enabled ? "已减少页面模糊与平滑滚动，可随时关闭" : "已恢复网页原有视觉效果");
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
    }
    return { mount, status, showCompactionReview };
  })();
