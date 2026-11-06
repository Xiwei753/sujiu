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
            const promptId = Branching.getCurrentPromptId();
            if (!promptId || promptId === "new_chat") {
                PromptPanel.status("先保存当前聊天，再执行摘要压缩");
                return;
            }
            const task = CompactionTask.create(promptId, Number(userSettings.keepRounds) || 8);
            const prompt = CompactionTask.compose(getCompressPrompt(), task);
            CompactionTask.save(task);
            Compression.prepareRunContext();
            state.activeCompressionPrompt = prompt;
            state.activeCompressionSnippet = deriveSnippet(getCompressPrompt());
            state.activeCompressionMarker = task.marker;
            state.activeCompressionKeepRounds = task.keepRounds;
            state.scopedCompression = null;
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
                    log.ui("Compression click sent; listening without a time limit");
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
            if (!isTargetRequest(detail?.url, "GENERATE") || !detail?.bodyText) return;
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
            if (!isTargetRequest(detail?.url, "GENERATE")) return;
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
            log.net("Summary response completed; ready for manual review", { length: snapshot.length });
            PromptPanel.status("AI Studio 已返回摘要。请先确认模型已经完成，再点击「读取已完成摘要」核对并准备清理。");
            Compression.finalizeRun();
        },

        stopTracking() {
            if (!state.compressionInProgress) {
                PromptPanel.status("当前没有正在监听的摘要请求");
                return;
            }
            Compression.finalizeRun();
            PromptPanel.status("已停止插件监听，AI Studio 正在生成的内容不会被中断；完成后可以手动读取摘要。");
        },

        async recover() {
            if (state.branchInProgress) {
                PromptPanel.status("正在清理旧消息，请先完成当前操作");
                return;
            }
            try {
                PromptPanel.status("正在从当前会话重新读取已保存的摘要……");
                await recoverLatestCompletedSummary();
                Compression.finalizeRun();
            } catch (error) {
                PromptPanel.status("尚未取得可用于清理的完整摘要：" + String(error?.message || error));
                log.ui("Manual summary recovery failed", String(error?.message || error));
            }
        },

        finalizeRun() {
            state.compressionRequestPending = false;
            state.compressionResponsePending = false;
            state.compressionInProgress = false;
            state.historyCapturePending = false;
            UI.setButtonLoading(false);
        },

        isCompressionPrompt(bodyText) {
            if (typeof bodyText !== "string") return false;
            if (state.activeCompressionMarker && bodyText.includes(state.activeCompressionMarker)) return true;
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
        // Keep the capture handlers active for as long as the model needs.
        // Recovery is a separate user-initiated step, so no timeout or retry.
        return true;
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
