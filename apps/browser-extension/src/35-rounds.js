const RoundSplitter = Object.freeze({
    partition(turns, keepRounds = 8) {
      if (!Array.isArray(turns) || !Number.isSafeInteger(keepRounds) || keepRounds < 1) {
        throw new Error("Invalid conversation history or keepRounds");
      }
      const rounds = [];
      let current = null;
      function closeRound() {
        if (!current) return;
        if (!current.some(turn => turn.role === "model")) {
          throw new Error("Conversation contains an incomplete response");
        }
        rounds.push(current);
      }
      for (const turn of turns) {
        if (!turn || typeof turn.role !== "string") {
          throw new Error("Conversation contains an invalid turn");
        }
        if (turn.role === "user") {
          closeRound();
          current = [turn];
        } else if (!current) {
          throw new Error("Conversation starts with a non-user turn");
        } else {
          current.push(turn);
        }
      }
      closeRound();
      if (rounds.length <= keepRounds) {
        throw new Error("Not enough completed rounds to compress");
      }
      const older = rounds.slice(0, -keepRounds).flat();
      const recent = rounds.slice(-keepRounds).flat();
      return { older, recent, olderRounds: rounds.length - keepRounds, recentRounds: keepRounds, totalRounds: rounds.length };
    }
  });
