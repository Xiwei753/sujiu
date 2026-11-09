const RoundSplitter = Object.freeze({
    // Pinned compaction memories can remain as standalone model messages
    // after their temporary tool prompts are removed. They are never counted
    // as conversational user/model rounds.
    textOf(turn) {
      return typeof turn?.text === "string" ? turn.text :
        JSON.stringify(turn?.entry ?? "");
    },
    isMemory(turn) {
      if (turn?.role !== "model") return false;
      const content = this.textOf(turn);
      if (/\[\[SUJIU_MEMORY_V1:[a-z0-9-]{8,80}\]\]/i.test(content)) return true;
      // Compatibility with early Sujiu summaries and their XML wrapper.
      // A positive match pins the result; uncertain replies are never
      // automatically relabeled merely because they look long.
      if (/(?:<|&lt;)\/?state_snapshot(?:>|&gt;)/i.test(content)) return true;
      const headings = ["【历史范围与时间锚点】","【稳定设定与约束】",
        "【人物与关系】","【关键事件时间线】","【重要状态变化】",
        "【本段历史截点】","【连续性校验】","【未解决线索与悬而未决的信息】",
        "【稳定设定】","【已发生的关键事件】",
        "【截至压缩截点的状态】","【时间线衔接】"];
      return headings.filter(heading => content.includes(heading)).length >= 2 ||
        /^(?:历史摘要|长期记忆|压缩记忆|剧情摘要|history summary|memory summary|conversation summary|story summary|context summary)\s*[:：]/i.test(content.trim());
    },
    isCompressionPrompt(turn) {
      if (turn?.role !== "user") return false;
      const content = this.textOf(turn);
      const excerpt = content.slice(0, 800);
      return /\[\[SUJIU_SCOPE_V2:[a-z0-9-]{8,80}\]\]/i.test(content) ||
        /(?:<|&lt;)state_snapshot(?:>|&gt;)/i.test(content) ||
        content.includes("Summarize the earlier role-play conversation") ||
        content.includes("请为这段长期对话制作按剧情时间线整理的精炼记忆") ||
        content.includes("请只根据本次请求中实际提供的历史记录") ||
        content.includes("你是负责长期会话连续性的记忆整理员") ||
        (content.includes("历史范围与时间锚点") && content.includes("关键事件时间线")) ||
        (/(?:摘要|总结|梳理|压缩|归纳|概括)/.test(excerpt) &&
         /(?:前文|历史|剧情|对话|聊天|情节|记忆|人物关系|时间线|之前发生|已有内容)/.test(excerpt)) ||
        (/(?:summari[sz]e|summary|recap|condense|compress(?:ion)?|memory snapshot)/i.test(excerpt) &&
         /(?:earlier|previous|prior|conversation|role.?play|history|chat|story|dialogue|context)/i.test(excerpt));
    },
    partition(turns, keepRounds = 8) {
      if (!Array.isArray(turns) || !Number.isSafeInteger(keepRounds) || keepRounds < 1) {
        throw new Error("Invalid conversation history or keepRounds");
      }
      const units = [];
      let current = null;
      function closeRound() {
        if (!current) return;
        if (!current.some(turn => turn.role === "model")) {
          throw new Error("Conversation contains an incomplete response");
        }
        // Tool-generated summary exchanges are archived history, not
        // normal roleplay turns. They must never consume any of the N recent
        // ordinary dialogue rounds kept during a later manual cleanup.
        units.push({ kind: (RoundSplitter.isCompressionPrompt(current[0]) ||
          current.some(turn => RoundSplitter.isMemory(turn))) ? "memory" : "round",
                     turns: current });
        current = null;
      }
      for (const turn of turns) {
        if (!turn || typeof turn.role !== "string") {
          throw new Error("Conversation contains an invalid turn");
        }
        if (turn.role === "user") {
          closeRound();
          current = [turn];
        } else if (this.isMemory(turn) && (!current ||
          (current.some(item => item.role === "model") &&
           !/\[\[SUJIU_SCOPE_V2:/i.test(current[0]?.text ||
               JSON.stringify(current[0]?.entry?.[0] ?? ""))))) {
          closeRound();
          units.push({ kind: "memory", turns: [turn] });
        } else if (!current) {
          throw new Error("Conversation starts with a non-user turn");
        } else {
          current.push(turn);
        }
      }
      closeRound();
      const regularRounds = units.filter(unit => unit.kind === "round").length;
      if (regularRounds <= keepRounds) {
        throw new Error("Not enough completed rounds to compress");
      }
      let seen = 0;
      let keepIndex = -1;
      for (let i = units.length - 1; i >= 0; i--) {
        if (units[i].kind === "round" && ++seen === keepRounds) {
          keepIndex = i;
          break;
        }
      }
      if (keepIndex < 0) throw new Error("Cannot identify recent turns");
      const older = units.slice(0, keepIndex).flatMap(unit => unit.turns);
      const recent = units.slice(keepIndex).flatMap(unit => unit.turns);
      return {
        older, recent,
        olderRounds: regularRounds - keepRounds,
        recentRounds: keepRounds,
        totalRounds: regularRounds,
        pinnedMemories: units.filter(unit => unit.kind === "memory").length
      };
    }
  });
