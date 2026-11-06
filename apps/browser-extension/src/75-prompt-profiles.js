const PromptProfiles = (() => {
    const sections = Object.freeze([
      { id: "system", title: "系统提示词" },
      { id: "character", title: "角色设定" },
      { id: "world", title: "世界设定" },
      { id: "persona", title: "用户人物卡" },
      { id: "author", title: "作者附注" }
    ]);
    const ids = sections.map(section => section.id);
    const idsSet = new Set(ids);
    const RESERVED = new Set(["__proto__", "prototype", "constructor"]);
    const MAX_PRESETS = 50;
    const MAX_FIELD_LENGTH = 100000;
    function blank() {
      return {
        fields: Object.fromEntries(ids.map(id => [id, ""])),
        enabled: Object.fromEntries(ids.map(id => [id, true])),
        order: ids.slice()
      };
    }
    function normalizedPreset(input) {
      const preset = blank();
      if (!input || typeof input !== "object" || Array.isArray(input)) return preset;
      // Read legacy presets where text fields were stored directly on the preset.
      const fields = input.fields && typeof input.fields === "object" ? input.fields : input;
      for (const id of ids) {
        if (typeof fields[id] === "string") preset.fields[id] = fields[id].slice(0, MAX_FIELD_LENGTH);
        if (typeof input.enabled?.[id] === "boolean") preset.enabled[id] = input.enabled[id];
      }
      if (Array.isArray(input.order)) {
        const order = [...new Set(input.order.filter(id => idsSet.has(id)))];
        preset.order = [...order, ...ids.filter(id => !order.includes(id))];
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
      return { version: 2, active, presets };
    }
    function compile(preset) {
      const p = normalizedPreset(preset);
      return p.order.filter(id => p.enabled[id] && p.fields[id].trim())
        .map(id => "[" + sections.find(s => s.id === id).title + "]\n" + p.fields[id].trim())
        .join("\n\n");
    }
    function shift(preset, id, delta) {
      const p = normalizedPreset(preset);
      const at = p.order.indexOf(id);
      const next = at + delta;
      if (at >= 0 && next >= 0 && next < p.order.length) {
        [p.order[at], p.order[next]] = [p.order[next], p.order[at]];
      }
      return p;
    }
    return Object.freeze({ sections, blank, normalize, normalizedPreset, compile, shift, maxPresets: MAX_PRESETS });
  })();
