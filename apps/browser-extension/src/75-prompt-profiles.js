const PromptProfiles = (() => {
    const LEGACY_SECTIONS = Object.freeze([
      { id: "system", title: "系统提示词" },
      { id: "character", title: "角色设定" },
      { id: "world", title: "世界设定" },
      { id: "persona", title: "用户人物卡" },
      { id: "author", title: "作者附注" }
    ]);
    const LEGACY_IDS = LEGACY_SECTIONS.map(item => item.id);
    const RESERVED = new Set(["__proto__", "prototype", "constructor"]);
    const MAX_PRESETS = 50;
    // A v2 preset could hold five fields of 100,000 characters each.
    // Do not truncate that content merely because it now has one editor.
    const MAX_TEXT_LENGTH = 550000;
    const MAX_LEGACY_FIELD_LENGTH = 100000;

    function blank() {
      return { text: "" };
    }
    function convertLegacy(input) {
      const source = input?.fields && typeof input.fields === "object" ? input.fields : input;
      const rawOrder = Array.isArray(input?.order) ? input.order : LEGACY_IDS;
      const order = [...new Set(rawOrder.filter(id => LEGACY_IDS.includes(id)))];
      order.push(...LEGACY_IDS.filter(id => !order.includes(id)));
      const sections = [], legacyDisabled = Object.create(null);
      for (const id of order) {
        const text = typeof source?.[id] === "string"
          ? source[id].slice(0, MAX_LEGACY_FIELD_LENGTH) : "";
        if (!text.trim()) continue;
        if (input?.enabled?.[id] === false) {
          // Keep inactive drafts in the exported preset, but never send
          // them to Gemini as newly enabled system instructions.
          legacyDisabled[id] = text;
        } else {
          const title = LEGACY_SECTIONS.find(item => item.id === id).title;
          sections.push("[" + title + "]\n" + text.trim());
        }
      }
      const result = { text: sections.join("\n\n").slice(0, MAX_TEXT_LENGTH) };
      if (Object.keys(legacyDisabled).length) result.legacyDisabled = legacyDisabled;
      return result;
    }
    function normalizedPreset(input) {
      if (typeof input === "string") return { text: input.slice(0, MAX_TEXT_LENGTH) };
      if (!input || typeof input !== "object" || Array.isArray(input)) return blank();
      if (typeof input.text !== "string") return convertLegacy(input);
      const preset = { text: input.text.slice(0, MAX_TEXT_LENGTH) };
      if (input.legacyDisabled && typeof input.legacyDisabled === "object" &&
          !Array.isArray(input.legacyDisabled)) {
        const disabled = Object.create(null);
        for (const id of LEGACY_IDS) {
          if (typeof input.legacyDisabled[id] === "string")
            disabled[id] = input.legacyDisabled[id].slice(0, MAX_LEGACY_FIELD_LENGTH);
        }
        if (Object.keys(disabled).length) preset.legacyDisabled = disabled;
      }
      return preset;
    }
    function normalize(source) {
      const presets = Object.create(null);
      if (source && typeof source === "object" && !Array.isArray(source) &&
          source.presets && typeof source.presets === "object" && !Array.isArray(source.presets)) {
        for (const [rawName, rawPreset] of Object.entries(source.presets)) {
          const name = rawName.trim().slice(0, 80);
          if (!name || RESERVED.has(name) || Object.hasOwn(presets, name)) continue;
          presets[name] = normalizedPreset(rawPreset);
          if (Object.keys(presets).length >= MAX_PRESETS) break;
        }
      }
      if (!Object.keys(presets).length) presets["默认"] = blank();
      const active = typeof source?.active === "string" && Object.hasOwn(presets, source.active)
        ? source.active : Object.keys(presets)[0];
      return { version: 3, active, presets };
    }
    function compile(preset) {
      return normalizedPreset(preset).text;
    }
    return Object.freeze({
      blank, normalize, normalizedPreset, compile,
      maxPresets: MAX_PRESETS, maxTextLength: MAX_TEXT_LENGTH
    });
  })();
