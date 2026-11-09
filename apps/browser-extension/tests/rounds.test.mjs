const test = (await import("node:test")).default;
  const assert = (await import("node:assert/strict")).default;
  const fs = await import("node:fs/promises");
  const path = await import("node:path");
  const vm = await import("node:vm");
  const source = await fs.readFile(new URL("../src/35-rounds.js",import.meta.url),"utf8");
  const splitter = vm.runInNewContext(source + "\nRoundSplitter", {});
  function make(count) {
    return Array.from({length:count}, (_,i)=> [
      {role:"user",text:"user "+i},
      {role:"model",text:"model "+i}
    ]).flat();
  }
  test("60 rounds → 52 old + 8 recent, preserving whole entries", () => {
    const source = make(60);
    const result = splitter.partition(source,8);
    assert.equal(result.totalRounds,60);
    assert.equal(result.olderRounds,52);
    assert.equal(result.recentRounds,8);
    assert.equal(result.older.length,104);
    assert.equal(result.recent.length,16);
    assert.equal(result.recent[0].text,"user 52");
    assert.equal(result.recent.at(-1),source.at(-1));
  });
  test("tool messages remain attached to their user/model round", () => {
    const source = make(10);
    source.splice(3,0,{role:"tool",text:"result"});
    const result = splitter.partition(source,2);
    assert.equal(result.recent.length,4);
    assert.equal(result.older.length,17);
  });
  test("reject incomplete rounds or insufficient history", () => {
    assert.throws(()=>splitter.partition([{role:"user",text:"incomplete"}],1));
    assert.throws(()=>splitter.partition(make(8),8));
    assert.throws(()=>splitter.partition(make(3),0));
  });

  test("old standalone marked memories are preserved separately from recent user rounds", () => {
    const source = make(5);
    source.splice(4, 0, {role:"model",text:"[[SUJIU_MEMORY_V1:12345678-1234]] earlier summary"});
    const result = splitter.partition(source, 2);
    assert.equal(result.totalRounds, 5);
    assert.equal(result.olderRounds, 3);
    assert.equal(result.pinnedMemories, 1);
    assert.ok(result.older.some(t => t.text?.includes("SUJIU_MEMORY_V1")));
    assert.equal(result.recent[0].text, "user 3");
  });
  test("a tool-generated marked summary remains paired but does not count as ordinary dialogue", () => {
    const source = make(4);
    source.splice(4, 0,
      {role:"user",text:"Summary task [[SUJIU_SCOPE_V2:12345678-1234]]"},
      {role:"model",text:"reasoning before snapshot"},
      {role:"model",text:"[[SUJIU_MEMORY_V1:12345678-1234]] pinned reply"});
    const result = splitter.partition(source, 2);
    assert.equal(result.totalRounds, 4);
    assert.equal(result.olderRounds, 2);
    assert.equal(result.pinnedMemories, 1);
    assert.ok(result.older.some(t => t.text?.includes("SUJIU_MEMORY_V1")));
  });

  test("unmarked result to a marked tool prompt does not consume recent ordinary dialogue", () => {
    const source = make(3);
    source.splice(4, 0,
      {role:"user", text:"[[SUJIU_SCOPE_V2:12345678-aaaa]] history task"},
      {role:"model", text:"Summary without permanent tag"});
    const result = splitter.partition(source, 2);
    assert.equal(result.totalRounds, 3);
    assert.equal(result.olderRounds, 1);
    assert.equal(result.pinnedMemories, 1);
    assert.equal(result.recent[0].text, "user 1");
  });

  test("request scope recognizes the marker in raw Google entry content", () => {
    const source = make(3);
    source.splice(2, 0,
      {role:"user", entry:[["[[SUJIU_SCOPE_V2:12345678-bbbb]]"]]},
      {role:"model", entry:[["[[SUJIU_MEMORY_V1:12345678-bbbb]]"]]});
    const result = splitter.partition(source, 1);
    assert.equal(result.totalRounds, 3);
    assert.equal(result.pinnedMemories, 1);
    assert.equal(result.recent[0].text, "user 2");
  });
