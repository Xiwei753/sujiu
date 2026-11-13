const test = (await import("node:test")).default;
  const assert = (await import("node:assert/strict")).default;
  const fs = await import("node:fs/promises");
  const path = await import("node:path");
  const vm = await import("node:vm");
  const source = await fs.readFile(new URL("../src/35-rounds.js",import.meta.url),"utf8");
  const splitter = vm.runInNewContext(source + "\nRoundSplitter", {});
  const markerSource = await fs.readFile(new URL("../src/25-compaction-task.js",import.meta.url),"utf8");
  const markers = vm.runInNewContext(markerSource + "\nCompactionTask", {});
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

  test("unmarked Chinese summary text is an ordinary round, not protected", () => {
    const source = make(3);
    source.splice(2, 0,
      {role:"user",text:"请总结之前的剧情，方便继续"},
      {role:"model",text:"【历史范围与时间锚点】前情【关键事件时间线】初遇"});
    const result = splitter.partition(source, 2);
    assert.equal(result.totalRounds, 4);
    assert.equal(result.olderRounds, 2);
    assert.equal(result.pinnedMemories, 0);
    assert.equal(result.recent[0].text, "user 1");
  });

test("XML state_snapshot alone is not a protection marker", () => {
    const source = make(3);
    source.splice(2, 0,
      {role:"user",text:"Summarize the earlier role-play conversation. Return only: <state_snapshot>"},
      {role:"model",text:"<state_snapshot>Historical memory</state_snapshot>"});
    const result = splitter.partition(source, 1);
    assert.equal(result.totalRounds, 4);
    assert.equal(result.pinnedMemories, 0);
    assert.equal(result.olderRounds, 3);
    assert.ok(result.older.some(turn => turn.text?.includes("<state_snapshot>Historical memory")));
  });

  test("Chinese summary headings alone do not protect messages", () => {
    const source = make(3);
    source.splice(2, 0,
      {role:"user",text:"请为这段长期对话制作按剧情时间线整理的精炼记忆"},
      {role:"model",text:"【稳定设定】主角已转学。\\n【已发生的关键事件】认识了新同学。"});
    const result = splitter.partition(source, 1);
    assert.equal(result.totalRounds, 4);
    assert.equal(result.pinnedMemories, 0);
  });

  test("user protection marker excludes their round from the recent-N count", () => {
    const source = make(4);
    source[2].text = "[[SUJIU_PROTECT_V1]] Personally important instruction";
    const split = splitter.partition(source, 1);
    assert.equal(split.totalRounds, 3);
    assert.equal(split.pinnedMemories, 1);
    assert.equal(split.olderRounds, 2);
    assert.ok(split.older.includes(source[2]));
    assert.ok(split.older.includes(source[3]));
    assert.equal(split.recent[0].text, "user 3");
  });

  test("protected model replies and all generated memory markers survive as separate archives", () => {
    const source = make(4);
    source.splice(4,0,
      {role:"model",text:"[[SUJIU_MEMORY_V1:12345678-aaaa]] First summary"},
      {role:"model",text:"[[SUJIU_MEMORY_V1:12345678-bbbb]] Second summary"},
      {role:"model",text:"[[SUJIU_PROTECT_V1]] Manual protected reply"});
    const split = splitter.partition(source,1);
    assert.equal(split.totalRounds,4);
    assert.equal(split.pinnedMemories,3);
    assert.equal(split.olderRounds,3);
    assert.equal(split.recent[0].text,"user 3");
  });

  test("real CompactionTask accepts explicit protection markers in either message role", () => {
    assert.equal(markers.isProtectedMessage("sujiu"),true);
    assert.equal(markers.isProtectedMessage("Keep this please: SuJiU"),true);
    assert.equal(markers.isProtectedMessage("My custom sujiu reminder"),true);
    assert.equal(markers.isProtectedMessage("[[SUJIU_PROTECT_V1]] My user text"),true);
    assert.equal(markers.isProtectedMessage("Prefix [[SUJIU_MEMORY_V1:12345678-abcd]] Model memory"),true);
    assert.equal(markers.isProtectedMessage("SUJIU_PROTECT_V1 without square brackets"),true);
    assert.equal(markers.isProtectedMessage("[[SUJIU_SCOPE_V2:12345678-abcd]] Disposable task"),true);
    assert.equal(splitter.hasProtectionMarker({
      role:"user",text:"[[SUJIU_SCOPE_V2:12345678-abcd]] Disposable task sujiu"
    }),false,"full scoped task marker overrides the literal token");
    assert.equal(markers.isProtectedMessage("【稳定设定】A【已发生的关键事件】B"),false);
    assert.equal(markers.isProtectedMessage("<state_snapshot>memory</state_snapshot>"),false);
  });

  test("literal sujiu protects a user-authored message from the compression round window", () => {
    const source = make(4);
    source[2].text="Important user note sujiu";
    const result=splitter.partition(source,1);
    assert.equal(result.totalRounds,3);
    assert.equal(result.pinnedMemories,1);
    assert.equal(result.recent[0].text,"user 3");
  });

  test("generated summary prompt asks the model to output literal sujiu", () => {
    const scoped={
      marker:"[[SUJIU_SCOPE_V2:12345678-abcd]]",
      promptId:"p",
      keepRounds:1
    };
    const prompt=markers.compose("Summarize",scoped);
    assert.ok(prompt.includes("摘要第一行单独输出这五个字母：sujiu"));
    assert.ok(prompt.includes("[[SUJIU_MEMORY_V1:12345678-abcd]]"));
    assert.equal(splitter.hasProtectionMarker({role:"user",text:prompt}),false);
  });
