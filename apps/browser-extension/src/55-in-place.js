    const InPlaceCompaction = (() => {
        let pending = null;
        let deleting = false;
        const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

        function normalizeType(turn) {
            const container = turn.querySelector(".chat-turn-container");
            if (container?.classList?.contains("user")) return "user";
            if (turn.classList?.contains("thought-activity-host")) return "thought";
            if (turn.querySelector("ms-thought-chunk, .thought-panel") &&
                !turn.querySelector(".turn-content ms-text-chunk, .turn-content ms-code-block, .turn-content .cmark-node")) {
                return "thought";
            }
            const raw = (turn.getAttribute("data-turn-type") ||
                turn.getAttribute("data-turn-role") ||
                turn.dataset?.turnType || turn.dataset?.turnRole || "").toLowerCase();
            if (raw === "user" || raw === "prompt") return "user";
            if (raw === "thought") return "thought";
            if (raw === "model" || raw === "text" || raw === "response") return "model";
            if (container?.classList?.contains("model") ||
                container?.classList?.contains("render") ||
                turn.querySelector(".turn-content, .virtual-scroll-container")) return "model";
            return "unknown";
        }

        // Use every rendered native turn, including thought blocks. An
        // incomplete / virtualized DOM is never safe for destructive changes.
        function collectGroups(turns) {
            const groups = [];
            for (const turn of turns) {
                const type = normalizeType(turn);
                if (type === "unknown") throw new Error("发现未识别的消息类型，已停止删除");
                if (type === "user") {
                    groups.push({ turns: [turn], hasModel: false, kind: "round" });
                } else {
                    const group = groups[groups.length - 1];
                    const hasMemory = type === "model" &&
                        CompactionTask.isPinnedSummary(turn.textContent || "");
                    const toolPrompt = group?.turns?.[0]?.textContent?.includes("[[SUJIU_SCOPE_V2:");
                    if (hasMemory && (!group || (group.hasModel && !toolPrompt))) {
                        groups.push({ turns: [turn], hasModel: true, kind: "memory" });
                        continue;
                    }
                    if (!group) throw new Error("历史消息的顺序异常");
                    group.turns.push(turn);
                    if (type === "model") group.hasModel = true;
                }
            }
            if (groups.some(group => !group.hasModel)) throw new Error("发现未完成的轮次，已停止删除");
            return groups;
        }

        function stage(snapshot, split, task = null) {
            const id = Branching.getCurrentPromptId();
            if (!id || id === "new_chat") throw new Error("保存会话后才能原地压缩");
            if (!snapshot?.trim()) throw new Error("摘要为空，无法压缩");
            // Previously generated memories are pinned. Never select
            // their own model replies for deletion in a later compression.
            const pinnedOlderRounds = [];
            const pinnedMemoryTexts = [];
            let roundIndex = -1;
            let currentUser = "";
            let currentHasModel = false;
            for (const turn of split.older || []) {
                if (turn.role === "user") {
                    roundIndex++;
                    currentUser = String(turn.text || "");
                    currentHasModel = false;
                } else if (turn.role === "model") {
                    const isPinned = CompactionTask.isPinnedSummary(turn.text || "");
                    if (isPinned) {
                        pinnedMemoryTexts.push(String(turn.text).trim());
                        const isStandalone = currentHasModel &&
                            !currentUser.includes("[[SUJIU_SCOPE_V2:");
                        if (!isStandalone && roundIndex >= 0 &&
                            !pinnedOlderRounds.includes(roundIndex)) {
                            pinnedOlderRounds.push(roundIndex);
                        }
                    }
                    currentHasModel = true;
                }
            }
            pending = {
                pinnedOlderRounds,
                pinnedMemoryTexts,
                promptId: id,
                snapshot: snapshot.trim(),
                olderRounds: split.olderRounds,
                recentRounds: split.recentRounds,
                totalRounds: split.totalRounds,
                marker: task?.marker || null,
                memoryMarker: task?.pinnedMarker || (task?.marker ?
                    CompactionTask.memoryMarker(task.marker) : null),
                legacy: Boolean(task?.legacy),
                createdAt: Date.now()
            };
            PromptPanel.showCompactionReview(pending);
            return pending;
        }

        function planManualCleanup(keepRounds = Number(userSettings.keepRounds) || 8) {
            const promptId = Branching.getCurrentPromptId();
            if (!promptId || promptId === "new_chat") {
                throw new Error("请先保存当前聊天");
            }
            if (!Number.isSafeInteger(keepRounds) || keepRounds < 1 || keepRounds > 100) {
                throw new Error("保留轮数必须是 1 到 100 的整数");
            }
            const turns = [...document.querySelectorAll("ms-chat-turn")];
            const groups = collectGroups(turns);
            const regular = groups.filter(group => group.kind !== "memory");
            if (regular.length <= keepRounds) {
                throw new Error("当前只有 " + regular.length +
                    " 轮完整对话，没有超出保留范围的历史");
            }
            const olderRounds = regular.length - keepRounds;
            const pinnedOlderRounds = [];
            for (let i = 0; i < olderRounds; i++) {
                if (regular[i].turns.some(turn => normalizeType(turn) === "model" &&
                    CompactionTask.isPinnedSummary(turn.textContent || ""))) {
                    pinnedOlderRounds.push(i);
                }
            }
            const plan = {
                manual: true,
                promptId, snapshot: "", marker: null, memoryMarker: null,
                totalRounds: regular.length, olderRounds, recentRounds: keepRounds,
                pinnedOlderRounds, pinnedMemoryTexts: [], legacy: false,
                // UI may virtualize or reorder chat nodes; never delete a
                // stale selection after the page has changed.
                domIds: turns.map(turn => turn.id)
            };
            return plan;
        }

        async function manualCleanup() {
            if (deleting) return;
            // This path is independent of compression / task markers.
            // A user may clear old history without producing any summary.
            const plan = planManualCleanup();
            return commit(plan);
        }

        async function verifyPersistedSummary(plan) {
            const capture = await fetchResolvedPromptThread(plan.promptId);
            if (!capture?.entries?.length) throw new Error("未能读取最新会话，停止删除");
            assertSupportedEntries(capture.entries, extractResolvedTurns(capture.entries));
            const turns = extractResolvedTurns(capture.entries);
            const users = turns.filter(turn => turn.role === "user");
            if (users.length !== plan.totalRounds + 1) {
                throw new Error("会话轮数与生成摘要前不一致，请刷新后重新生成摘要");
            }
            const latest = findSavedSummary(turns, plan.promptId);
            if (latest.marker !== plan.marker ||
                latest.keepRounds !== plan.recentRounds ||
                latest.snapshot.trim() !== plan.snapshot.trim() ) {
                throw new Error("Google 端摘要或保留轮数与预览不一致，拒绝删除");
            }
            const previous = turns.slice(0, latest.lastUserIndex);
            const split = RoundSplitter.partition(previous, plan.recentRounds);
            if (split.totalRounds !== plan.totalRounds || split.olderRounds !== plan.olderRounds) {
                throw new Error("原聊天已经变化，请重新读取摘要再清理");
            }
            return { capture, turns };
        }

        function preflightDom(plan) {
            const turns = [...document.querySelectorAll("ms-chat-turn")];
            const groups = collectGroups(turns);
            const regular = groups.filter(group => group.kind !== "memory");
            const expected = plan.totalRounds + (plan.manual ? 0 : 1);
            if (regular.length !== expected) {
                throw new Error("网页历史轮次已改变（检测到 " +
                    regular.length + "，预期 " + expected + "），请重新操作");
            }
            if (plan.manual && (turns.length !== plan.domIds.length ||
                turns.some((turn, index) => !turn.id || turn.id !== plan.domIds[index]))) {
                throw new Error("网页聊天节点在准备期间发生变化，请重新点击清理");
            }
            const pinned = new Set(plan.pinnedOlderRounds || []);
            const getRole = turn => normalizeType(turn);
            for (const index of pinned) {
                if (index < 0 || index >= plan.olderRounds ||
                    !regular[index]?.turns?.some(turn =>
                        getRole(turn) === "model" &&
                        CompactionTask.isPinnedSummary(turn.textContent || ""))) {
                    throw new Error("历史压缩记忆与网页消息位置不一致，拒绝删除");
                }
            }

            // Regular old exchanges can be removed; completed summary rounds
            // are pinned in their entirety, so their visible memory survives.
            const targetTurns = [];
            const ignoredOldThoughts = new Set();
            for (let i = 0; i < plan.olderRounds; i++) {
                if (pinned.has(i)) continue;
                for (const turn of [...regular[i].turns].reverse()) {
                    if (getRole(turn) === "thought" && !turn.querySelector(
                        "ms-chat-turn-options button, ms-chat-turn-options span.ms-button-icon-symbol"
                    )) {
                        // It may disappear with its parent response; avoid
                        // independently targeting an unclickable thought node.
                        ignoredOldThoughts.add(turn.id);
                    } else {
                        targetTurns.push(turn);
                    }
                }
            }

            // The newest summary is pinned by a separate marker on the MODEL
            // reply. The tool's temporary USER instruction and independent
            // thought cards are eligible for cleanup; never target its reply.
            const lastGroup = regular[regular.length - 1];
            let skippedThoughts = 0;
            if (plan.marker && plan.memoryMarker) {
                const user = lastGroup.turns[0];
                const memoryNodes = lastGroup.turns.filter(turn =>
                    getRole(turn) === "model" &&
                    (turn.textContent || "").includes(plan.memoryMarker));
                if (getRole(user) !== "user" ||
                    !(user.textContent || "").includes(plan.marker) ||
                    !memoryNodes.length) {
                    throw new Error("当前压缩摘要没有可见的永久标识，拒绝清理工具消息");
                }
                // Thought cards embedded inside a model response cannot be
                // removed independently without risking the summary itself.
                for (const turn of lastGroup.turns.slice(1)) {
                    if (getRole(turn) !== "thought") continue;
                    if (turn.querySelector("ms-chat-turn-options button, ms-chat-turn-options span.ms-button-icon-symbol")) {
                        targetTurns.push(turn);
                    } else {
                        skippedThoughts++;
                    }
                }
                targetTurns.push(user);
            }

            const targetIds = targetTurns.map(turn => turn.id);
            if (targetIds.some(id => !id) || new Set(targetIds).size !== targetIds.length) {
                throw new Error("待删除消息缺少稳定 ID，拒绝执行");
            }
            const targetSet = new Set(targetIds);
            const protectedTurns = turns.filter(turn =>
                !targetSet.has(turn.id) && !ignoredOldThoughts.has(turn.id));
            if (protectedTurns.some(turn => !turn.id || targetSet.has(turn.id))) {
                throw new Error("保护内容与删除范围发生冲突");
            }
            if (targetTurns.some(turn => !turn.querySelector(
                "ms-chat-turn-options button, ms-chat-turn-options span.ms-button-icon-symbol"
            ))) {
                throw new Error("待删除消息缺少独立的原生删除操作，拒绝执行");
            }
            const deletedUserCount = targetTurns.filter(turn => getRole(turn) === "user").length;
            const deletedOldRounds = plan.olderRounds - pinned.size;
            return {
                targetIds,
                protectedIds: protectedTurns.map(turn => turn.id),
                deletedUserCount, deletedOldRounds, pinnedCount: pinned.size,
                skippedThoughts, groups: groups.length, items: targetIds.length
            };
        }

        async function findNativeDeleteItem() {
            for (let attempt = 0; attempt < 12; attempt++) {
                const items = [...document.querySelectorAll(
                    ".cdk-overlay-container .mat-mdc-menu-content button, .cdk-overlay-container [role=menuitem], .cdk-overlay-pane button"
                )];
                const target = items.find(item => {
                    if (item.disabled || item.getAttribute("aria-disabled") === "true") return false;
                    const icon = item.querySelector("mat-icon, .material-symbols-outlined, [data-mat-icon-name]");
                    const label = [
                        item.getAttribute("aria-label"),
                        icon?.getAttribute("fonticon"),
                        icon?.getAttribute("data-mat-icon-name"),
                        icon?.textContent,
                        item.textContent
                    ].filter(Boolean).join(" ").toLowerCase();
                    return /(?:delete|trash|删除|移除)/i.test(label);
                });
                if (target) return target;
                await sleep(80);
            }
            return null;
        }

        async function deleteOne(id) {
            const turn = document.getElementById(id);
            if (!turn || !turn.isConnected) return;
            const button = turn.querySelector("ms-chat-turn-options span.ms-button-icon-symbol") ||
                turn.querySelector("ms-chat-turn-options button.mat-mdc-menu-trigger") ||
                turn.querySelector("ms-chat-turn-options button");
            if (!button) {
                if (normalizeType(turn) === "thought") {
                    throw new Error("思考块 " + id + " 尚未随回复被删除，也没有独立的原生删除菜单");
                }
                throw new Error("消息 " + id + " 缺少原生操作菜单");
            }
            button.click();
            const item = await findNativeDeleteItem();
            if (!item) throw new Error("找不到 Google 原生删除选项，已停止");
            item.focus?.();
            item.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true }));
            item.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true }));
            item.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, cancelable: true }));
            item.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, cancelable: true }));
            item.click();
            // A further confirmation dialog is deliberately not bypassed.
            for (let i = 0; i < 100; i++) {
                if (!document.getElementById(id)) return;
                await sleep(100);
            }
            throw new Error("消息 " + id + " 删除后未从页面消失，已停止");
        }

        async function commit(selectedPlan = null) {
            if (deleting) return;
            const plan = selectedPlan || pending;
            if (!plan) throw new Error("请先读取摘要或使用「直接清理旧历史」");
            if (Branching.getCurrentPromptId() !== plan.promptId) {
                throw new Error("会话已切换，请重新读取摘要");
            }
            // Preflight still verifies the staged summary and protected
            // boundary; only post-deletion server polling is removed.
            if (!plan.manual) await verifyPersistedSummary(plan);
            const preview = preflightDom(plan);
            const prompt = (plan.manual ? "手动清理（无需摘要）：准备删除 " : "准备一次性清理 ") +
                preview.deletedOldRounds + " 轮旧历史" +
                (preview.pinnedCount ? "（已保护 " + preview.pinnedCount + " 条旧压缩记忆）" : "") +
                (plan.memoryMarker ? "，并清理本次工具指令及可独立删除的思考块" : "") +
                "。保留最近 " + plan.recentRounds + " 轮与已标记摘要。" +
                (plan.manual ? "本次不依赖新摘要；未标记的旧摘要可能按普通历史删除。" : "") +
                (preview.skippedThoughts ? "另有 " + preview.skippedThoughts +
                    " 个无法独立删除的思考块会保留。" : "") +
                "删除操作使用 AI Studio 原生消息菜单，Google 会自行自动保存。全部删除后统一等待保存响应。该操作不可撤销，继续？";
            if (!confirm(prompt)) return;
            deleting = true;
            state.branchInProgress = true;
            let processed = 0;
            const saveBaseline = SaveMonitor.attempts;
            let lastDeletionStartedAt = 0;
            try {
                const protectedSet = new Set(preview.protectedIds);
                for (const id of preview.targetIds) {
                    if (Branching.getCurrentPromptId() !== plan.promptId) {
                        throw new Error("清理过程中会话已经切换");
                    }
                    lastDeletionStartedAt = Date.now();
                    await deleteOne(id);
                    processed++;
                    // An Angular cascading delete may remove the summary
                    // alongside its temporary user prompt. Stop immediately
                    // if that happens, instead of deleting the rest blindly.
                    for (const protectedId of protectedSet) {
                        if (!document.getElementById(protectedId)) {
                            throw new Error("原生删除联动影响了受保护消息 " +
                                protectedId + "，已立即停止。请刷新并核对摘要");
                        }
                    }
                    PromptPanel.status("批量清理中 " + processed + "/" + preview.items +
                        "，原生页面将自行处理自动保存");
                }
                // The old per-turn path never invoked Save explicitly.
                // AI Studio's native delete action triggers UpdatePrompt
                // automatically. Observe one quiet period after the batch.
                PromptPanel.status("批量删除完成，等待 Google 原生自动保存请求结束……");
                const autosave = await SaveMonitor.waitForNativeSave(saveBaseline, lastDeletionStartedAt);
                if (!autosave.observed) {
                    PromptPanel.status("页面删除已完成，但未观察到 UpdatePrompt 自动保存。" +
                        "请确认 Google 页面已同步，保存前不要刷新。");
                    return;
                }
                if (!autosave.ok) {
                    PromptPanel.status("Google 原生自动保存返回 " + autosave.status +
                        "，请检查网络和页面状态，暂勿刷新。");
                    return;
                }
                if (!selectedPlan) {
                    pending = null;
                    PromptPanel.showCompactionReview(null);
                }
                PromptPanel.status("批量删除已完成；观察到 " + autosave.attempts +
                    " 次原生 UpdatePrompt 响应，最后状态 " + autosave.status +
                    "。这表示保存请求有成功响应，刷新后可核对实际云端历史。" +
                    (preview.skippedThoughts ? "部分无法独立删除的思考块已保留。" : ""));
            } catch (error) {
                PromptPanel.status("批量清理处理了 " + processed + "/" + preview.items +
                    " 个页面消息后停止：" + String(error?.message || error) +
                    "。请刷新检查服务器实际状态");
                throw error;
            } finally {
                deleting = false;
                state.branchInProgress = false;
            }
        }

        function cancel() {
            if (deleting) return;
            pending = null;
            PromptPanel.showCompactionReview(null);
            PromptPanel.status("已取消清理。摘要已生成在当前聊天中，原始历史仍在。");
        }

        return Object.freeze({
            stage, commit, manualCleanup, planManualCleanup, cancel,
            collectGroups, preflightDom, get pending() { return pending; }
        });
    })();
