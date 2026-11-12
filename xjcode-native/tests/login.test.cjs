const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const script = fs.readFileSync(require('node:path').join(__dirname, '../src/login.js'), 'utf8').replace('__XJCODE_NONCE__', '"fixture-nonce"');
function browser(keys, options = {}) {
  const calls = [], frames = [], statuses = [], listeners = {};
  let poll, stopped = false;
  const window = { ipc: { postMessage: value => frames.push(JSON.parse(value)) }, addEventListener: (name, fn) => { listeners[name] = fn; } };
  window.top = window;
  const element = { style: {}, set textContent(value) { statuses.push(value); } };
  const context = {
    window, location: { origin: options.origin || 'https://ai.xjtu.edu.cn' },
    document: { body: { appendChild() {} }, cookie: 'x-csrf-token=fixture-csrf', getElementById: () => element },
    setInterval: callback => { poll = callback; return 1; },
    clearInterval: () => { stopped = true; },
    setTimeout, clearTimeout, AbortController: options.noAbort ? undefined : AbortController,
    fetch: async (url, init) => {
      const action = new URL(url, 'https://ai.xjtu.edu.cn').searchParams.get('Action');
      calls.push(action);
      assert.equal(init.credentials, 'same-origin');
      assert.equal(init.headers['x-csrf-token'], 'fixture-csrf');
      if (action === 'GetSelfInfo' && options.failIdentity && calls.filter(s => s === 'GetSelfInfo').length <= options.failIdentity) throw new Error('network');
      if (action === 'ListAccessKey' && options.forbiddenList) return {ok:false, status:403};
      if (action === 'ListAccessKey' && options.failList) throw new Error('network');
      if (action === 'CreateAccessKey' && options.failCreate) throw new Error('timeout');
      const Result = action === 'GetSelfInfo' ? { User: { ID: 'account', TenantID: 'tenant' } }
        : action === 'ListAccessKey' ? (options.listResult || { Items: keys, Total: keys.length })
        : { AccessKey: { AccessKey: 'created', SecretKey: 'encrypted' } };
      return { ok: true, json: async () => ({ Result }) };
    }
  };
  vm.runInNewContext(script, context);
  return { calls, frames, statuses, signal: name => listeners[name]?.(), tick: async () => { if (poll && !stopped) await poll(); } };
}
test('reuse newest active key and pass account + tenant separately', async () => {
  const b = browser([
    { AccessKey: 'old', SecretKey: 'encrypted-old', Status: 1, CreatedTime: '2025' },
    { AccessKey: 'new', SecretKey: 'encrypted-new', Status: 1, CreatedTime: '2026' }
  ]);
  await b.tick(); await b.tick();
  assert.deepEqual(b.calls, ['GetSelfInfo', 'ListAccessKey']);
  assert.equal(b.frames.length, 1);
  assert.equal(b.frames[0].access_key, 'new');
  assert.equal(b.frames[0].tenant_id, 'tenant');
  assert.equal(b.frames[0].account_id, 'account');
});
test('create only when capacity permits', async () => {
  const b = browser([]);
  await b.tick(); await b.tick();
  assert.deepEqual(b.calls, ['GetSelfInfo', 'ListAccessKey', 'CreateAccessKey']);
  assert.equal(b.frames[0].access_key, 'created');
});
test('two unusable keys never trigger create, delete or activate', async () => {
  const b = browser([{ Status: 0 }, { Status: 0 }]);
  await b.tick();
  assert.deepEqual(b.calls, ['GetSelfInfo', 'ListAccessKey']);
  assert.equal(b.frames.length, 0);
  assert.ok(b.statuses.some(s => s.includes('两个不可用密钥')));
});
test('uncertain creation is never repeated', async () => {
  const b = browser([], { failCreate: true });
  await b.tick(); await b.tick();
  assert.equal(b.calls.filter(s => s === 'CreateAccessKey').length, 1);
  assert.equal(b.frames.length, 0);
});
test('non-portal origins cannot access IAM or IPC', async () => {
  const b = browser([], { origin: 'https://sso.xjtu.edu.cn' });
  await b.tick();
  assert.equal(b.calls.length, 0);
  assert.equal(b.frames.length, 0);
});

test('works without AbortSignal.timeout and does not require AbortController', async () => {
  const b = browser([{ AccessKey:'a', SecretKey:'encrypted', Status:1 }], {noAbort:true});
  await b.tick(); assert.equal(b.frames.length,1); assert.equal(b.frames[0].access_key,'a');
});
test('identity failures give actionable feedback and a later success still hands off', async () => {
  const b=browser([{AccessKey:'a',SecretKey:'encrypted',Status:1}],{failIdentity:3});
  await b.tick();await b.tick();await b.tick();assert.ok(b.statuses.some(s=>s.includes('已经进入交晓智首页')));
  await b.tick();assert.equal(b.frames.length,1);
});
test('failed read never creates a key and handoff events show safe guidance', async () => {
  const b=browser([],{failList:true});await b.tick();await b.tick();
  assert.deepEqual(b.calls,['GetSelfInfo','ListAccessKey']);assert.equal(b.frames.length,0);
  b.signal('xjcode-handoff-slow');b.signal('xjcode-handoff-failed');
  assert.ok(b.statuses.some(s=>s.includes('等待 XJCode')));assert.ok(b.statuses.some(s=>s.includes('未能接收')));
});

test('portal empty list represented by null with explicit zero can create once', async () => {
  const b=browser([], {listResult:{Items:null,Total:0}});
  await b.tick(); await b.tick();
  assert.deepEqual(b.calls,['GetSelfInfo','ListAccessKey','CreateAccessKey']);
  assert.equal(b.frames.length,1);
});
test('ambiguous or inconsistent empty list never creates credentials', async () => {
  for (const listResult of [{Items:null},{Items:null,Total:2},{Items:[],Total:-1},{Items:'bad',Total:0}]) {
    const b=browser([],{listResult}); await b.tick(); await b.tick();
    assert.deepEqual(b.calls,['GetSelfInfo','ListAccessKey']);
    assert.equal(b.frames.length,0);
    assert.ok(b.statuses.some(s=>s.includes('列表格式异常')));
  }
});
test('403 and uncertain create have distinct safe guidance', async () => {
  const denied=browser([],{forbiddenList:true});await denied.tick();
  assert.ok(denied.statuses.some(s=>s.includes('拒绝读取密钥列表（403）')));
  assert.deepEqual(denied.calls,['GetSelfInfo','ListAccessKey']);
  const uncertain=browser([],{failCreate:true});await uncertain.tick();await uncertain.tick();
  assert.ok(uncertain.statuses.some(s=>s.includes('创建结果未确认')));
  assert.equal(uncertain.calls.filter(x=>x==='CreateAccessKey').length,1);
});
