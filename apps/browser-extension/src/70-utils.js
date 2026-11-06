    function extractMutationPromptId(root) {
        const resourceName = typeof root?.[0] === "string" ? root[0] : "";
        const match = resourceName.match(/prompts\/([^/?#]+)/);
        return match ? match[1] : null;
    }

    function buildMutationThreadEntry(turn, sequence = 0) {
        if (isReusablePromptThreadEntry(turn?.entry)) {
            const entry = deepClone(turn.entry);
            entry[0] = turn?.text || "";
            entry[8] = turn?.role === "model" ? "model" : "user";
            if (Array.isArray(entry[21]) && typeof entry[21][0] === "string" && /^prompts\//.test(entry[21][0])) {
                entry[21] = null;
            }
            entry[32] = buildEntryTimestamp(sequence);
            return entry;
        }
        return buildCreatePromptEntry(turn, sequence);
    }

    function isReusablePromptThreadEntry(entry) {
        return Array.isArray(entry) && entry.length >= 33 && typeof entry[8] === "string";
    }

    function buildCreatePromptEntry(turn, sequence = 0) {
        const text = turn?.text || "";
        const role = turn?.role === "model" ? "model" : "user";
        const entry = new Array(33).fill(null);
        entry[0] = text;
        entry[8] = role;
        const tokenEstimate = estimateTokenCount(text);
        if (tokenEstimate > 0) {
            entry[18] = tokenEstimate;
        }
        entry[32] = buildEntryTimestamp(sequence);
        return entry;
    }

    function buildEntryTimestamp(sequence = 0) {
        const timestampMs = Date.now() + Math.max(0, Number(sequence) || 0);
        const seconds = Math.floor(timestampMs / 1000);
        const nanos = (timestampMs % 1000) * 1_000_000;
        return [seconds, nanos];
    }

    function estimateTokenCount(text) {
        if (!text) return 0;
        return Math.max(1, Math.round(text.length / 4));
    }

    function deepClone(value) {
        return value == null ? value : JSON.parse(JSON.stringify(value));
    }

    function safeParseJSON(text) {
        if (typeof text !== "string") return null;
        try {
            return JSON.parse(text);
        } catch {
            return null;
        }
    }

    function buildSnippetVariants(snippet) {
        if (typeof snippet !== "string" || !snippet) {
            return [];
        }
        const variants = [snippet];
        const escaped = escapeForJSON(snippet);
        if (escaped && escaped !== snippet) {
            variants.push(escaped);
        }
        return variants;
    }

    function escapeForJSON(text) {
        try {
            const encoded = JSON.stringify(text);
            return typeof encoded === "string" ? encoded.slice(1, -1) : text;
        } catch {
            return text;
        }
    }

    function matchByRegex(text, pattern) {
        if (!pattern) return null;
        try {
            const regex = new RegExp(pattern, "s");
            const result = text.match(regex);
            return result ? result[0] : null;
        } catch (err) {
            log.net("Invalid snapshot regex", err);
            return null;
        }
    }
