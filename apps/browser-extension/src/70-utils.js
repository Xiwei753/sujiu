    function extractMutationPromptId(root) {
        const resourceName = typeof root?.[0] === "string" ? root[0] : "";
        const match = resourceName.match(/prompts\/([^/?#]+)/);
        return match ? match[1] : null;
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
