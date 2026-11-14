import { test } from 'node:test';
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';

const hooks = [];
const subscribed = [];
const events = {
    CHAT_CHANGED: 'chat_id_changed',
    APP_READY: 'app_ready',
    CHARACTER_MESSAGE_RENDERED: 'character_message_rendered',
    MESSAGE_RECEIVED: 'message_received',
};
const formatter = {
    stage:{ AFTER_REGEX:'afterRegex', AFTER_MARKDOWN:'afterMarkdown' },
    order:{ LATEST:100 },
    addHook(fn,opts){ hooks.push({fn,...opts}); },
};
const ctxt = {
    extensionSettings:{},
    messageFormatter: formatter,
    characters:[{data:{extensions:{sujiu:{structured_chat:true}}}}],
    characterId:0,
    eventTypes:events,
    eventSource:{on(type){subscribed.push(type);}},
    saveSettingsDebounced(){}
};
globalThis.SillyTavern = {getContext:()=>ctxt};
globalThis.document = {readyState:'complete',querySelector:()=>null,getElementById:()=>null};
const entry = pathToFileURL(resolve(import.meta.dirname,'../index.js')).href;
await import(entry);

test('official SillyTavern eventTypes subscriptions register',()=>{
    assert.deepEqual(subscribed.slice().sort(),Object.values(events).sort());
});

test('extension registers both pipeline hooks',()=>{
    assert.equal(hooks.length,2);
    assert.deepEqual(hooks.map(x=>x.stage),['afterRegex','afterMarkdown']);
});

test('formatter uses the raw JSON, returns bubbles, never modifies stored chat data',()=>{
    const raw=JSON.stringify({sujiu_schema:'1',segments:[{speaker:'御坂美琴',kind:'dialogue',text:'早上好！'}],state:{time:'6月16日 08:10',inventory:['课本']},continuity:{facts:['测试']}});
    const meta={messageId:4,isSystem:false,isUser:false,isReasoning:false};
    const unchanged = hooks[0].fn(raw,meta);
    assert.equal(unchanged,raw);
    const rendered=hooks[1].fn('<p>should be replaced</p>',meta);
    assert.match(rendered,/御坂美琴/);
    assert.match(rendered,/sujiu-continuity/);
    assert.doesNotMatch(rendered,/should be replaced/);
    assert.equal(unchanged,raw);
});

test('non-target messages pass through without mutation',()=>{
    const meta={messageId:4,isSystem:false,isUser:true,isReasoning:false};
    hooks[0].fn('hello',meta);
    assert.equal(hooks[1].fn('<p>hello</p>',meta),'<p>hello</p>');
});

test('disabling extension bypasses formatter',()=>{
    ctxt.extensionSettings.sujiu_structured_chat.enabled = false;
    const meta={messageId:1,isSystem:false,isUser:false,isReasoning:false};
    hooks[0].fn('{"sujiu_schema":"1"}',meta);
    assert.equal(hooks[1].fn('<p>raw</p>',meta),'<p>raw</p>');
});
