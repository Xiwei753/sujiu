import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseSujiu } from '../parser.mjs';
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
