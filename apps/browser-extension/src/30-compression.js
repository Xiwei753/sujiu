    const Compression = {
        run() {
            if (state.compressionInProgress) {
                log.ui("Compression already running");
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
            enableSendButton();
            sendCompressionRequest(textarea).then(sent => {
                if (sent) {
                    log.ui("Compression request sent");
                    return;
                }
                log.ui("Failed to trigger compression request; aborting this run");
                Compression.finalizeRun();
            });
        },

        prepareRunContext() {
            state.lastPromptHistory = null;
            state.lastPromptHistoryUpdatedAt = 0;
            state.pendingBranchTurns = null;
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
            log.net("Compression snapshot extracted, rebuilding conversation");
            rebuildConversation(snapshot).catch(err => {
                log.net("Failed to rebuild conversation", err);
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

    function enableSendButton() {
        const sendButton = UI.findSendButton();
        if (sendButton) {
            sendButton.removeAttribute("disabled");
            sendButton.removeAttribute("aria-disabled");
            sendButton.disabled = false;
            sendButton.classList.remove("disabled");
        }
    }

    function delay(ms) {
        return new Promise(resolve => setTimeout(resolve, ms));
    }

    function nextFrame() {
        return new Promise(resolve => requestAnimationFrame(() => resolve()));
    }

    async function waitForCompressionRequestCapture(timeoutMs = 1800) {
        const deadline = Date.now() + timeoutMs;
        while (Date.now() < deadline) {
            if (!state.compressionInProgress) {
                return false;
            }
            if (state.compressionResponsePending || !state.compressionRequestPending) {
                return true;
            }
            await delay(50);
        }
        return state.compressionResponsePending || !state.compressionRequestPending;
    }

    function dispatchPointerClickSequence(el) {
        if (!el || typeof el.dispatchEvent !== "function") return false;
        const doc = el.ownerDocument || document;
        const view = doc.defaultView || window;
        const rect = typeof el.getBoundingClientRect === "function"
            ? el.getBoundingClientRect()
            : { left: 0, top: 0, width: 0, height: 0 };
        const centerX = rect.left + (rect.width || 0) / 2;
        const centerY = rect.top + (rect.height || 0) / 2;
        const eventSteps = [
            { type: "pointerdown", pointer: true, buttons: 1, detail: 0 },
            { type: "mousedown", buttons: 1, detail: 1 },
            { type: "pointerup", pointer: true, buttons: 0, detail: 0 },
            { type: "mouseup", buttons: 0, detail: 1 },
            { type: "click", buttons: 0, detail: 1 }
        ];

        try {
            if (typeof el.focus === "function") {
                el.focus({ preventScroll: true });
            }
        } catch {
            if (typeof el.focus === "function") {
                el.focus();
            }
        }

        for (const step of eventSteps) {
            const init = {
                bubbles: true,
                cancelable: true,
                composed: true,
                view,
                clientX: centerX,
                clientY: centerY,
                button: 0,
                buttons: step.buttons,
                detail: step.detail
            };
            let event;
            if (step.pointer && typeof view.PointerEvent === "function") {
                event = new view.PointerEvent(step.type, {
                    ...init,
                    pointerId: 1,
                    pointerType: "mouse",
                    isPrimary: true
                });
            } else {
                event = new view.MouseEvent(step.type, init);
            }
            el.dispatchEvent(event);
        }
        return true;
    }

    function triggerKeyboardSend(textarea) {
        const doc = textarea.ownerDocument || document;
        const view = doc.defaultView || window;
        const keyOptions = {
            key: "Enter",
            code: "Enter",
            keyCode: 13,
            bubbles: true,
            cancelable: true,
            view
        };
        if (env.isMac) {
            keyOptions.metaKey = true;
        } else {
            keyOptions.ctrlKey = true;
        }
        textarea.dispatchEvent(new view.KeyboardEvent("keydown", keyOptions));
        textarea.dispatchEvent(new view.KeyboardEvent("keyup", keyOptions));
        log.ui("Prompt send attempted via keyboard shortcut fallback");
    }

    async function sendPromptWithRetries(textarea, options = {}) {
        const clickPlan = [
            { label: "immediate", delayMs: 0, useFrame: false },
            { label: "next-frame", delayMs: 0, useFrame: true },
            { label: "delay-80ms", delayMs: 80, useFrame: false },
            { label: "delay-220ms", delayMs: 220, useFrame: false }
        ];
        const verifyFn = typeof options.verifyFn === "function" ? options.verifyFn : null;
        const labelPrefix = options.labelPrefix || "Prompt send";
        const abortCheck = typeof options.abortCheck === "function" ? options.abortCheck : null;

        for (const step of clickPlan) {
            if (step.useFrame) {
                await nextFrame();
            } else if (step.delayMs > 0) {
                await delay(step.delayMs);
            }
            if (abortCheck && abortCheck()) {
                return false;
            }

            enableSendButton();
            const sendButton = UI.findSendButton();
            if (!sendButton) {
                log.ui("Send button missing before click attempt", `${labelPrefix} ${step.label}`);
                continue;
            }

            dispatchPointerClickSequence(sendButton);
            log.ui(`${labelPrefix} click attempted`, step.label);
            if (!verifyFn || await verifyFn()) {
                log.ui(`${labelPrefix} confirmed`, step.label);
                return true;
            }
        }

        triggerKeyboardSend(textarea);
        if (!verifyFn || await verifyFn()) {
            log.ui(`${labelPrefix} confirmed via keyboard fallback`);
            return true;
        }
        return false;
    }

    async function sendCompressionRequest(textarea) {
        return sendPromptWithRetries(textarea, {
            labelPrefix: "Compression request",
            verifyFn: () => waitForCompressionRequestCapture(),
            abortCheck: () => !state.compressionInProgress
        });
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
    // Conversation rebuild & branch creation
    // ---------------------------
