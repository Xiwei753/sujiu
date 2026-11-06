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
                if (type === "user") groups.push({ turns: [turn], hasModel: false });
                else {
                    const group = groups[groups.length - 1];
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
            pending = {
                promptId: id,
                snapshot: snapshot.trim(),
                olderRounds: split.olderRounds,
                recentRounds: split.recentRounds,
                totalRounds: split.totalRounds,
                marker: task?.marker || null,
                legacy: Boolean(task?.legacy),
                createdAt: Date.now()
            };
            PromptPanel.showCompactionReview(pending);
            return pending;
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
            if (groups.length !== plan.totalRounds + 1) {
                throw new Error("网页未完整加载所有历史轮次（检测到 " +
                    groups.length + "，预期 " + (plan.totalRounds + 1) + "），拒绝删除");
            }
            // Delete response and attached thought blocks before each user
            // prompt. Some native UI builds remove thought blocks together
            // with the model response.
            const selected = groups.slice(0, plan.olderRounds)
                .flatMap(group => [...group.turns].reverse());
            const protectedTurns = groups.slice(plan.olderRounds).flatMap(group => group.turns);
            const ids = selected.map(turn => turn.id);
            if (ids.some(id => !id) || new Set(ids).size !== ids.length) {
                throw new Error("部分旧消息没有稳定 ID，无法安全删除");
            }
            if (protectedTurns.some(turn => ids.includes(turn.id))) {
                throw new Error("删除范围与保留范围重叠");
            }
            if (selected.some(turn => normalizeType(turn) !== "thought" &&
                !turn.querySelector("ms-chat-turn-options button, ms-chat-turn-options span.ms-button-icon-symbol"))) {
                throw new Error("部分消息缺少 Google 原生删除菜单，拒绝开始清理");
            }
            return { targetIds: ids, groups: groups.length, items: selected.length };
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

        async function commit() {
            if (deleting) return;
            const plan = pending;
            if (!plan) throw new Error("没有可应用的摘要");
            if (Branching.getCurrentPromptId() !== plan.promptId) {
                throw new Error("会话已切换，需重新压缩");
            }
            await verifyPersistedSummary(plan);
            const preview = preflightDom(plan);
            if (!confirm("即将在当前聊天中删除最旧的 " + plan.olderRounds +
                    " 轮（包括关联思考内容），保留最近 " + plan.recentRounds +
                    " 轮和刚生成的摘要。此操作无法撤销，确认继续？")) return;
            deleting = true;
            state.branchInProgress = true;
            let processed = 0;
            try {
                for (const id of preview.targetIds) {
                    if (Branching.getCurrentPromptId() !== plan.promptId) {
                        throw new Error("删除过程中会话发生切换");
                    }
                    await deleteOne(id);
                    processed++;
                    PromptPanel.status("正在原地清理：" + processed + "/" + preview.items);
                }
                const after = await fetchResolvedPromptThread(plan.promptId);
                if (!after?.entries?.length) throw new Error("无法验证 Google 端保存结果");
                const remaining = extractResolvedTurns(after.entries);
                const remainingUsers = remaining.filter(t => t.role === "user").length;
                if (remainingUsers !== plan.recentRounds + 1) {
                    throw new Error("已执行删除，但云端仍有 " + remainingUsers +
                        " 条用户消息；请刷新确认，避免重复操作");
                }
                pending = null;
                PromptPanel.showCompactionReview(null);
                PromptPanel.status("原地压缩完成，聊天地址保持不变。建议刷新后核对历史。");
            } catch (error) {
                PromptPanel.status("已处理 " + processed + "/" + preview.items + " 条后停止：" +
                    String(error?.message || error) + "。请刷新检查现有历史。");
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

        return Object.freeze({ stage, commit, cancel, collectGroups, preflightDom, get pending() { return pending; } });
    })();
