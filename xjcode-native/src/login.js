(() => {
  if (window.top !== window || location.origin !== 'https://ai.xjtu.edu.cn') return;
  const nonce = __XJCODE_NONCE__;
  let busy = false;
  let identityFailures = 0;
  const status = text => {
    let el = document.getElementById('xjcode-login-status');
    if (!el && document.body) {
      el = document.createElement('div'); el.id = 'xjcode-login-status';
      el.style.cssText = 'position:fixed;bottom:12px;left:12px;right:12px;z-index:2147483647;padding:14px;background:#10243a;color:white;border-radius:8px;font:14px Microsoft YaHei,sans-serif';
      document.body.appendChild(el);
    }
    if (el) el.textContent = text;
  };
  window.addEventListener('xjcode-handoff-failed', () => status('网页已登录，但 XJCode 未能接收登录信息。请关闭此窗口，回到 PowerShell 重新运行 xjcode login；若仍失败，请检查安全软件是否阻止本机通信。'));
  window.addEventListener('xjcode-handoff-slow', () => status('网页已登录，正在等待 XJCode 接收登录信息；请先不要关闭窗口。'));
  const api = async (action, body = {}) => {
    const csrf = document.cookie.split(';').map(s => s.trim()).find(s => s.startsWith('x-csrf-token='));
    const controller = typeof AbortController === 'function' ? new AbortController() : null;
    let timeout;
    try {
      const request = (async () => {
        const response = await fetch('/api/iam?Action=' + action + '&Version=2024-12-25', {
          method: 'POST', credentials: 'same-origin', redirect: 'error',
          headers: {'content-type': 'application/json', 'x-top-region': 'cn-north-1', 'x-csrf-token': csrf ? decodeURIComponent(csrf.slice(13)) : ''},
          body: JSON.stringify(body), ...(controller ? {signal: controller.signal} : {})
        });
        if (!response.ok) throw new Error('HTTP_' + response.status);
        const result = await response.json();
        if (result.ResponseMetadata?.Error) throw new Error('IAM_ERROR');
        return result.Result;
      })();
      const deadline = new Promise((_, reject) => { timeout = setTimeout(() => { if (controller) controller.abort(); reject(new Error('TIMEOUT')); }, 20000); });
      return await Promise.race([request, deadline]);
    } finally { if (timeout) clearTimeout(timeout); }
  };
  const timer = setInterval(async () => {
    if (busy || !document.body) return;
    busy = true;
    let user;
    try { user = (await api('GetSelfInfo'))?.User; } catch {
      identityFailures++;
      if (identityFailures >= 3) status('正在等待交晓智确认登录。如果已经进入交晓智首页但长时间停留，请检查校园网或学校 VPN，再重新运行 xjcode login。');
      busy = false; return;
    }
    if (!user?.ID || !user?.TenantID) {
      identityFailures++;
      if (identityFailures >= 3) status('正在等待交晓智确认登录。如果已经进入交晓智首页但长时间停留，请检查校园网或学校 VPN，再重新运行 xjcode login。');
      busy = false; return;
    }
    clearInterval(timer);
    status('校园登录成功，正在为 XJCode 配置凭据…');
    try {
      let result;
      try { result = await api('ListAccessKey'); }
      catch (error) { throw new Error(error.message === 'HTTP_403' ? 'KEY_LIST_FORBIDDEN' : 'KEY_LIST_FAILED'); }
      // The portal itself accepts a null Items field for an empty list.
      // Only treat it as empty when the reported total explicitly confirms zero.
      if (!result || (!Array.isArray(result.Items) && !(result.Items == null && result.Total != null && Number(result.Total) === 0))) throw new Error('KEY_LIST_FORMAT');
      if (result.Total != null && (!Number.isSafeInteger(Number(result.Total)) || Number(result.Total) < 0)) throw new Error('KEY_LIST_FORMAT');
      const keys = result.Items || [];
      let key = keys.filter(k => Number(k.Status) === 1 && k.SecretKey && k.AccessKey)
        .sort((a,b) => String(b.CreatedTime).localeCompare(String(a.CreatedTime)) || String(a.AccessKey).localeCompare(String(b.AccessKey)))[0];
      if (!key) {
        if (Math.max(keys.length, Number(result.Total) || 0) >= 2) throw new Error('KEY_LIMIT');
        // A creation with an uncertain result must never be retried in this window.
        try { key = (await api('CreateAccessKey', {Status: 1}))?.AccessKey; }
        catch { throw new Error('KEY_CREATE_UNKNOWN'); }
      }
      if (!key?.AccessKey || !key?.SecretKey) throw new Error('KEY_FORMAT');
      window.ipc.postMessage(JSON.stringify({nonce, account_id: user.ID, tenant_id: user.TenantID, access_key: key.AccessKey, encrypted_secret: key.SecretKey}));
      status('已提交登录信息，正在等待 XJCode 确认；请先不要关闭窗口。');
    } catch (e) {
      const messages = {
        KEY_LIMIT: '已有两个不可用密钥。请在交晓智密钥管理页面处理后重新登录；XJCode 未删除或启用任何旧密钥。',
        KEY_LIST_FORBIDDEN: '交晓智拒绝读取密钥列表（403）。请在交晓智确认账号权限，或联系学校平台管理员。',
        KEY_LIST_FAILED: '读取交晓智密钥列表失败，本次没有创建密钥。请检查校园网或 VPN，稍后重新登录。',
        KEY_LIST_FORMAT: '交晓智返回的密钥列表格式异常，本次没有创建密钥。请联系 XJCode 管理员。',
        KEY_CREATE_UNKNOWN: '新密钥创建结果未确认。请先检查交晓智密钥管理页面，再重新登录；本次不会自动重试创建。',
        KEY_FORMAT: '交晓智返回的密钥信息不完整。请先检查交晓智密钥管理页面，再重新登录。'
      };
      status(messages[e?.message] || '凭据配置失败。请查看交晓智密钥管理页面，并联系 XJCode 管理员。');
    }
  }, 1500);
})();
