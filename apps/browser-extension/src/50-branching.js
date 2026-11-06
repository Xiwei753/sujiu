    // Kept as a stable prompt locator. Compaction never creates a new prompt.
    const Branching = Object.freeze({
        getCurrentPromptId() {
            const match = (location.pathname || "").match(/\/prompts\/([^/?#]+)/);
            return match ? match[1] : null;
        }
    });

