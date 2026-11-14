import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseSujiu, parseRoleplayText } from '../parser.mjs';
import { renderSujiu } from '../renderer.mjs';

const fixture = {
  sujiu_schema: '1',
  segments: [
    { speaker:'旁白',kind:'narration',text:'雨落在窗台上。' },
    { speaker:'御坂美琴',kind:'dialogue',text:'我叫御坂美琴。\n有什么需要帮忙的吗？' },
    { speaker:'初春饰利',kind:'action',text:'收起笔记本。' },
  ],
  state: { time:'6月16日 08:12',location:'常盘台',physical:'正常',mental:'平静',inventory:['书包','笔'],time_marker:'【sujiu:时标】Y0-06-16' },
  continuity: {nearby:['御坂美琴'],'thoughts':['担心今天的考试'],plot:'刚开场'}
};

test('complete structured message parses and renders', () => {
  const actual = parseSujiu(JSON.stringify(fixture));
  assert.equal(actual.complete, true);
  assert.equal(actual.segments.length, 3);
  const html = renderSujiu(actual);
  assert.match(html, /sujiu-speaker-name">御坂美琴/);
  assert.match(html, /sujiu-narration/);
  assert.match(html, /<details class="sujiu-continuity">/);
  assert.match(html, /持有物品/);
  assert.match(html, /6月16日 08:12/);
  assert.doesNotMatch(html, /【sujiu:时标】/); // internal bookkeeping remains in JSON, not visible text
});

test('streaming JSON yields previous bubbles and a partial last bubble', () => {
  const raw = JSON.stringify(fixture);
  const first = raw.slice(0,raw.indexOf('收起笔记本。') + 2);
  const p = parseSujiu(first);
  assert.equal(p.complete,false);
  assert.equal(p.segments.length,3);
  assert.equal(p.segments[0].text,'雨落在窗台上。');
  assert.ok(p.segments[2].text.startsWith('收起'));
  assert.match(renderSujiu(p),/生成中/);
});

test('HTML and script-like model content stays escaped', () => {
  const input=structuredClone(fixture);
  input.segments[1].speaker='<img src=x onerror=alert(1)>';
  input.segments[1].text='<script>alert(1)</script> & <b>Hello</b>';
  input.continuity.thoughts=['<svg onload=alert(2)>'];
  const html=renderSujiu(parseSujiu(JSON.stringify(input)));
  assert.doesNotMatch(html,/<script>/);
  assert.doesNotMatch(html,/<img src=x/);
  assert.doesNotMatch(html,/<svg onload=/);
  assert.match(html,/&lt;script&gt;/);
  assert.match(html,/&lt;svg/);
});

test('ordinary text is passed through unchanged by parser', () => {
  assert.equal(parseSujiu('今天的天气不错。'),null);
  assert.equal(parseSujiu('一个对象：{ "abc": 2 }'),null);
});

test('code fenced JSON still parses', () => {
  assert.equal(parseSujiu('```json\n'+JSON.stringify(fixture)+'\n```').complete,true);
});

test('malformed trailing JSON still yields bubbles', () => {
  const raw=JSON.stringify(fixture);
  const bad=raw.slice(0,raw.indexOf('"state":')) + '"state":{oops';
  const p=parseSujiu(bad);
  assert.equal(p.complete,false);
  assert.equal(p.segments.length,3);
});

test('plain non-specific JSON is not treated as sujiu schema', () => {
  assert.equal(parseSujiu('{"hello":"world"}'),null);
});


test('Android example: multi-speaker text becomes three speech bubbles', () => {
  const raw = '你的声音落在讲台上，清晰利落。\n同\n同学A\n月守……是那个新同学？\n同\n同学B\n从外面转来的大小姐？还是温度使。\n窗边的御坂美琴轻轻挑了挑眉，目光从你身上扫过。\n📚\n老师\n月守同学的能力测试成绩相当出色。';
  const data = parseRoleplayText(raw);
  assert.ok(data);
  assert.deepEqual(data.segments.map(s => [s.speaker, s.kind]), [
    ['旁白','narration'],['同学A','dialogue'],['同学B','dialogue'],
    ['旁白','narration'],['老师','dialogue'],
  ]);
  const html = renderSujiu(data);
  assert.equal((html.match(/class="sujiu-speech"/g) || []).length, 3);
  assert.equal((html.match(/class="sujiu-narration"/g) || []).length, 2);
  assert.doesNotMatch(html, /<p>同<\/p>/);
});

test('plain text fallback handles explicit and decorated speaker names', () => {
  for (const raw of ['老师：欢迎。', '📚\n老师\n欢迎。',
    '**御坂美琴**\n好呀。', '【月守】\n“请多指教。”']) {
    assert.equal(parseRoleplayText(raw)?.segments[0]?.kind, 'dialogue', raw);
  }
});

test('unlabelled prose and unrelated data keep the ordinary renderer', () => {
  for (const raw of ['今天天气很好。','她坐在窗边，看着旁边的同学。',
    '{"sujiu_schema":"1","segments":[','时间：今天\n地点：教室']) {
    assert.equal(parseRoleplayText(raw), null, raw);
  }
});

test('plain speech stays escaped, and consecutive actor bubbles are compact', () => {
  const html = renderSujiu(parseRoleplayText('老师：<img src=x onerror=alert(1)> & 小心'));
  assert.doesNotMatch(html, /<img src=x/);
  assert.match(html, /&lt;img/);
  assert.match(html, /&amp;/);
  const repeated = renderSujiu({complete:true,segments:[
    {speaker:'老师',kind:'dialogue',text:'第一句。'},
    {speaker:'老师',kind:'dialogue',text:'第二句。'},
    {speaker:'同学A',kind:'dialogue',text:'收到。'},
  ]});
  assert.equal((repeated.match(/sujiu-speaker-name/g) || []).length, 2);
  assert.equal((repeated.match(/sujiu-speaker-continued/g) || []).length, 1);
});
