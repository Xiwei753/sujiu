const AutoPrune = (() => {
    const MIN_INTERVAL_MS = 30000;
    let checking = false;
    let lastCheckAt = 0;
    let lastAttemptedId = "";
    function shouldCheck(settings, stateValue, now, lastAt) {
      return Boolean(settings.autoCompact &&
        Number.isSafeInteger(settings.autoThreshold) &&
        settings.autoThreshold >= 10 &&
        Number.isSafeInteger(settings.keepRounds) &&
        settings.keepRounds >= 1 &&
        settings.autoThreshold > settings.keepRounds &&
        !stateValue.compressionInProgress &&
        !stateValue.branchInProgress &&
        now - lastAt >= MIN_INTERVAL_MS);
    }
    async function evaluate() {
      const now = Date.now();
      if (checking || !shouldCheck(userSettings, state, now, lastCheckAt)) return;
      if (!/\/prompts\/[^/?#]+/.test(location.pathname) || location.pathname.includes("/new_chat")) return;
      const promptId = Branching.getCurrentPromptId();
      if (!promptId || promptId === "new_chat") return;
      const textarea = UI.findInput();
      if (!textarea || (typeof textarea.value === "string" && textarea.value.trim())) return;
      checking = true;
      lastCheckAt = now;
      try {
        // A native model reply can be committed slightly after the network stream closes.
        await delay(1000);
        if (Branching.getCurrentPromptId() !== promptId || state.compressionInProgress || state.branchInProgress) return;
        const capture = await fetchResolvedPromptThread(promptId);
        if (!capture?.entries?.length) return;
        const turns = extractResolvedTurns(capture.entries);
        assertSupportedEntries(capture.entries, turns);
        const result = RoundSplitter.partition(turns, Number(userSettings.keepRounds));
        if (result.totalRounds < Number(userSettings.autoThreshold)) return;
        if (lastAttemptedId === promptId) return;
        if (Branching.getCurrentPromptId() !== promptId) return;
        const currentInput = UI.findInput();
        if (!currentInput || (typeof currentInput.value === "string" && currentInput.value.trim())) return;
        lastAttemptedId = promptId;
        PromptPanel.status("检测到 " + result.totalRounds + " 轮，达到阈值；正在创建压缩分支");
        Compression.run();
      } catch (err) {
        PromptPanel.status("自动清理暂未执行：" + String(err?.message || err));
        log.net("Auto compaction check aborted:", String(err?.message || err));
      } finally {
        checking = false;
      }
    }
    return {
      onCompletedResponse(detail) {
        if (!detail || detail.status < 200 || detail.status >= 300) return;
        if (state.compressionInProgress || state.compressionResponsePending || state.branchInProgress) return;
        if (!shouldCheck(userSettings, state, Date.now(), lastCheckAt)) return;
        // Deferred so the original response handler is never blocked.
        void evaluate();
      },
      shouldCheck
    };
  })();
