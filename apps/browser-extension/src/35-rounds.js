const RoundSplitter = Object.freeze({
    // Pinned compaction memories can remain as standalone model messages
    // after their temporary tool prompts are removed. They are never counted
    // as conversational user/model rounds.
    isMemory(turn) {
      if (turn?.role !== "model") return false;
      const content = typeof turn.text === "string" ? turn.text :
        JSON.stringify(turn.entry?.[0] ?? "");
      return /\[\[SUJIU_MEMORY_V1:[a-z0-9-]{8,80}\]\]/i.test(content);
    },
    isCompressionPrompt(turn) {
      if (turn?.role !== "user") return false;
      const content = typeof turn.text === "string" ? turn.text :
        JSON.stringify(turn.entry?.[0] ?? "");
      return /\[\[SUJIU_SCOPE_V2:[a-z0-9-]{8,80}\]\]/i.test(content);
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
        units.push({ kind: this.isCompressionPrompt(current[0]) ? "memory" : "round",
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
