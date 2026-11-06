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
