(() => {
  if (window.top !== window || location.origin !== 'https://ai.xjtu.edu.cn') return;
  const nonce = __XJCODE_NONCE__;
  let busy = false;
  const status = text => {
    let el = document.getElementById('xjcode-login-status');
    if (!el && document.body) {
      el = document.createElement('div'); el.id = 'xjcode-login-status';
      el.style.cssText = 'position:fixed;bottom:12px;left:12px;right:12px;z-index:2147483647;padding:14px;background:#10243a;color:white;border-radius:8px;font:14px Microsoft YaHei,sans-serif';
      document.body.appendChild(el);
    }
    if (el) el.textContent = text;
  };
  const api = async (action, body = {}) => {
    const csrf = document.cookie.split(';').map(s => s.trim()).find(s => s.startsWith('x-csrf-token='));
    const response = await fetch('/api/iam?Action=' + action + '&Version=2024-12-25', {
      method: 'POST', credentials: 'same-origin', redirect: 'error',
      headers: {'content-type': 'application/json', 'x-top-region': 'cn-north-1', 'x-csrf-token': csrf ? decodeURIComponent(csrf.slice(13)) : ''},
      body: JSON.stringify(body), signal: AbortSignal.timeout(20000)
    });
    if (!response.ok) throw new Error('HTTP_' + response.status);
    const result = await response.json();
    if (result.ResponseMetadata?.Error) throw new Error('IAM_ERROR');
    return result.Result;
  };
  const timer = setInterval(async () => {
    if (busy || !document.body) return;
    busy = true;
    let user;
    try { user = (await api('GetSelfInfo'))?.User; } catch { busy = false; return; }
    if (!user?.ID || !user?.TenantID) { busy = false; return; }
    clearInterval(timer);
    status('校园登录成功，正在为 XJCode 配置凭据…');
    try {
      const result = await api('ListAccessKey');
      if (!Array.isArray(result.Items)) throw new Error('KEY_FORMAT');
      const keys = result.Items;
      let key = keys.filter(k => Number(k.Status) === 1 && k.SecretKey && k.AccessKey)
        .sort((a,b) => String(b.CreatedTime).localeCompare(String(a.CreatedTime)) || String(a.AccessKey).localeCompare(String(b.AccessKey)))[0];
      if (!key) {
        if (Math.max(keys.length, Number(result.Total) || 0) >= 2) throw new Error('KEY_LIMIT');
        // A creation with an uncertain result must never be retried in this window.
        key = (await api('CreateAccessKey', {Status: 1}))?.AccessKey;
      }
      if (!key?.AccessKey || !key?.SecretKey) throw new Error('KEY_FORMAT');
      window.ipc.postMessage(JSON.stringify({nonce, account_id: user.ID, tenant_id: user.TenantID, access_key: key.AccessKey, encrypted_secret: key.SecretKey}));
      status('凭据已安全交给 XJCode，请回到 PowerShell 选择工作区和模型。');
    } catch (e) {
      const message = e.message === 'KEY_LIMIT' ? '已有两个不可用密钥。请在交晓智密钥管理页面处理后重新登录；XJCode 未删除或启用任何旧密钥。' : '凭据配置失败或请求结果不明。请检查交晓智密钥管理页面后重新登录；本次不会自动重试创建密钥。';
      status(message);
    }
  }, 1500);
})();
