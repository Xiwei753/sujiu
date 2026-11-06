    const Branching = {
        async branchFromHere() {
            if (state.branchInProgress) {
                log.ui("Compression branch already running");
                return;
            }
            log.net("Attempting to create compressed prompt");
            state.branchInProgress = true;
            try {
                const expected = state.pendingBranchTurns?.map(turn => ({ role: turn.role, text: turn.text })) || [];
                const promptChange = await Branching.branchFromHereViaCreatePrompt();
                let valid = false;
                for (let attempt = 0; attempt < 4; attempt += 1) {
                    if (attempt) await delay(450);
                    const capture = await fetchResolvedPromptThread(promptChange.promptId);
                    const actual = capture?.entries ? extractResolvedTurns(capture.entries) : [];
                    if (actual.length === expected.length && actual.every((turn, index) =>
                        turn.role === expected[index].role && turn.text === expected[index].text)) {
                        valid = true;
                        break;
                    }
                }
                if (!valid) {
                    throw new Error("Created branch " + promptChange.promptId +
                        " but could not verify its saved history. Check /prompts/" + promptChange.promptId);
                }
                log.ui("Compressed branch verified; navigating");
                location.assign(`${location.origin}/prompts/${promptChange.promptId}`);
            } catch (err) {
                log.net("Compressed branch creation failed", err);
                alert("素酒压缩分支未完成验证： " + err.message);
                state.pendingBranchTurns = null;
                throw err;
            } finally {
                state.branchInProgress = false;
            }
        },

        async branchFromHereViaCreatePrompt() {
            const previousPromptId = Branching.getCurrentPromptId();
            if (!previousPromptId) {
                throw new Error("current prompt id not found");
            }
            if (!state.pendingBranchTurns?.length) {
                throw new Error("no pending turns available for branch creation");
            }
            const capture = await ensureResolvedPromptThread(previousPromptId);
            const body = buildDirectBranchCreateBody(capture, state.pendingBranchTurns);
            if (!body) {
                throw new Error("unable to build CreatePrompt branch payload");
            }
            const authHeader = await buildSapisidAuthHeader();
            if (!authHeader) {
                throw new Error("unable to build SAPISIDHASH auth header");
            }

            const pendingBranchTurns = state.pendingBranchTurns;
            const apiKey = getGoogleApiKey();
            state.pendingBranchTurns = null;
            try {
                const response = await fetch(`${Config.NETWORK.RPC_ORIGIN}${Config.TARGETS.CREATE_PROMPT}`, {
                    method: "POST",
                    credentials: "include",
                    headers: {
                        "content-type": "application/json+protobuf",
                        "x-goog-api-key": apiKey,
                        "x-goog-authuser": state.googleAuthUser || "0",
                        authorization: authHeader
                    },
                    body
                });
                const bodyText = await response.text();
                if (!response.ok) {
                    throw new Error(`CreatePrompt failed: HTTP ${response.status}`);
                }
                const parsed = safeParseJSON(bodyText);
                const promptId = Branching.pickCreatedPromptId(parsed, previousPromptId);
                if (!promptId) {
                    throw new Error("CreatePrompt succeeded but no prompt id was returned");
                }
                return {
                    promptId,
                    via: "direct-create-prompt"
                };
            } catch (err) {
                state.pendingBranchTurns = pendingBranchTurns;
                throw err;
            }
        },

        pickCreatedPromptId(payload, previousPromptId = null) {
            const topLevelPrompt = typeof payload?.[0] === "string" ? payload[0] : "";
            const topLevelMatch = topLevelPrompt.match(/^prompts\/([^/?#]+)/);
            if (topLevelMatch && (!previousPromptId || topLevelMatch[1] !== previousPromptId)) {
                return topLevelMatch[1];
            }
            const promptIds = collectPromptIds(payload);
            return promptIds.find(id => id && id !== previousPromptId) || null;
        },

        getCurrentPromptId() {
            const path = location.pathname || "";
            const match = path.match(/\/prompts\/([^/?#]+)/);
            return match ? match[1] : null;
        }
    };

    // ---------------------------
    // UI module (toolbar button)
    // ---------------------------
