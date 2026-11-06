    const Compression = {
        run() {
            if (state.compressionInProgress || state.branchInProgress) {
                log.ui("Compression or deletion already running");
                return;
            }
            if (InPlaceCompaction.pending) {
                PromptPanel.status("请先确认或取消已经生成的摘要");
                return;
            }
            const textarea = UI.findInput();
            if (!textarea) {
                log.ui("Input box not found, cannot run compression");
                return;
            }
            const prompt = getCompressPrompt() + "\n\nOnly summarize the completed dialogue BEFORE the last " + (Number(userSettings.keepRounds) || 8) + " full user/model exchanges. Do not summarize those last exchanges; they will be preserved verbatim.";
            Compression.prepareRunContext();
            state.activeCompressionPrompt = prompt;
            state.activeCompressionSnippet = deriveSnippet(prompt);
            state.compressionInProgress = true;
            state.compressionRequestPending = true;
            state.compressionResponsePending = false;
            UI.setButtonLoading(true);
            setTextareaValue(textarea, prompt);
            textarea.focus();
            textarea.dispatchEvent(new InputEvent("input", { bubbles: true, data: prompt, inputType: "insertText" }));
            textarea.dispatchEvent(new Event("change", { bubbles: true }));
            sendCompressionRequest().then(sent => {
                if (sent) {
                    log.ui("Compression request sent");
                    return;
                }
                log.ui("Compression request was not confirmed; no retry attempted");
                PromptPanel.status("未确认摘要请求。为避免误触停止按钮，插件已停止监听；请检查当前聊天是否仍在生成。");
                Compression.finalizeRun();
            }).catch(error => {
                log.ui("Compression single click failed", error);
                PromptPanel.status("摘要发送失败：" + String(error?.message || error));
                Compression.finalizeRun();
            });
        },

        prepareRunContext() {
            state.lastPromptHistory = null;
            state.lastPromptHistoryUpdatedAt = 0;
            state.historyCapturePending = true;
            state.compressionRequestPending = false;
            state.compressionResponsePending = false;
        },

        handleRequestEvent(event) {
            const detail = event?.detail;
            if (!detail?.bodyText) return;
            const isCompressionBody = Compression.isCompressionPrompt(detail.bodyText);

            if (!state.historyCapturePending && !state.compressionInProgress) {
                log.net("GenerateContent request observed outside compression run");
                return;
            }

            if (isCompressionBody && state.historyCapturePending) {
                state.historyCapturePending = false;
                const payload = safeParseJSON(detail.bodyText);
                if (payload) {
                    state.lastPromptHistory = payload;
                    state.lastPromptHistoryUpdatedAt = Date.now();
                    log.net("Stored prompt history snapshot for compression", {
                        ...describePromptHistory(payload)
                    });
                } else {
                    log.net("Compression request detected but prompt history parse failed");
                }
            } else if (state.historyCapturePending && !isCompressionBody) {
                log.net("Compression history capture pending, skipping non-compression request");
            }

            if (isCompressionBody && state.compressionInProgress) {
                state.compressionRequestPending = false;
                state.compressionResponsePending = true;
                log.net("Compression GenerateContent request captured");
            } else {
                log.net("Regular GenerateContent request observed");
            }
        },

        handleResponseEvent(event) {
            if (!state.compressionInProgress || !state.compressionResponsePending) {
                log.net("GenerateContent response event ignored", {
                    compressionInProgress: state.compressionInProgress,
                    compressionResponsePending: state.compressionResponsePending
                });
                return;
            }
            const detail = event?.detail;
            if (!detail?.bodyText) {
                log.net("GenerateContent response missing bodyText");
                Compression.finalizeRun();
                return;
            }
            const data = safeParseJSON(detail.bodyText);
            if (!data) {
                log.net("Failed to parse compression response");
                Compression.finalizeRun();
                return;
            }
            const snapshot = extractStateSnapshot(data);
            if (!snapshot) {
                log.net("No snapshot found in compression response");
                Compression.finalizeRun();
                return;
            }
            log.net("Compression snapshot extracted; preparing in-place deletion preview");
            prepareInPlaceCompaction(snapshot).catch(err => {
                log.net("Failed to prepare in-place deletion", err.message);
                alert("素酒压缩失败： " + err.message);
            });
            Compression.finalizeRun();
        },

        finalizeRun() {
            state.compressionRequestPending = false;
            state.compressionResponsePending = false;
            state.compressionInProgress = false;
            state.historyCapturePending = false;
            UI.setButtonLoading(false);
        },

        isCompressionPrompt(bodyText) {
            if (typeof bodyText !== "string") {
                return false;
            }
            const snippet = state.activeCompressionSnippet || deriveSnippet(getCompressPrompt());
            if (!snippet) {
                return bodyText.includes(Config.PROMPTS.SNIPPET);
            }
            const variants = buildSnippetVariants(snippet);
            for (const candidate of variants) {
                if (candidate && bodyText.includes(candidate)) {
                    return true;
                }
            }
            return false;
        }
    };

    function delay(ms) {
        return new Promise(resolve => setTimeout(resolve, ms));
    }

    async function waitForCompressionRequestCapture(timeoutMs = 12000) {
        const deadline = Date.now() + timeoutMs;
        while (Date.now() < deadline) {
            if (!state.compressionInProgress) return false;
            if (state.compressionResponsePending || !state.compressionRequestPending) return true;
            await delay(100);
        }
        return state.compressionResponsePending || !state.compressionRequestPending;
    }

    function isStopControl(button) {
        const icon = button.querySelector?.("mat-icon, .material-symbols-outlined");
        const label = [
            button.getAttribute?.("aria-label"),
            button.getAttribute?.("title"),
            button.getAttribute?.("iconname"),
            icon?.textContent,
            button.textContent
        ].filter(Boolean).join(" ").toLowerCase();
        return /(^|[^a-z])(stop|cancel|interrupt|abort)([^a-z]|$)|停止|中断生成|取消生成/.test(label);
    }

    async function sendCompressionRequest() {
        // Wait for the site's reactive input state to update. Never retry
        // a click: the original Run control may turn into Stop.
        await new Promise(resolve => requestAnimationFrame(resolve));
        if (!state.compressionInProgress) return false;
        const button = UI.findSendButton();
        if (!button || button.disabled || button.getAttribute?.("aria-disabled") === "true" || isStopControl(button)) {
            PromptPanel.status("未发送：原生运行按钮尚未就绪或正在生成。请检查输入框里的摘要指令后手动点击一次运行。");
            log.ui("Compression not auto-sent: Run button not ready");
            return false;
        }
        // One native click. No synthetic pointer sequence, force-enabling
        // the button, keyboard fallback, or second click on missed requests.
        button.click();
        log.ui("Compression request: single native Run click");
        PromptPanel.status("已点击运行一次，等待 AI Studio 接收请求；不会自动再次点击");
        return waitForCompressionRequestCapture();
    }

    function setTextareaValue(textarea, value) {
        if ("value" in textarea) {
            const descriptor = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value");
            const setter = descriptor?.set;
            if (setter) {
                setter.call(textarea, value);
                return;
            }
            textarea.value = value;
        } else {
            textarea.textContent = value;
        }
    }

    // ---------------------------
    // History processing
    // ---------------------------
