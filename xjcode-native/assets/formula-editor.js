(() => {
  'use strict';
  const $ = id => document.getElementById(id);
  const source = $('source'), status = $('status'), error = $('error');
  const storageKey = 'xjcode-formula-draft-v1';
  const templates = [
    ['分式', '\\frac{#}{b}', 'a'], ['上下标', '#_{i}^{2}', 'x'],
    ['根号', '\\sqrt{#}', 'x'], ['偏导', '\\frac{\\partial #}{\\partial x}', 'f'],
    ['积分', '\\int_{a}^{b} #\\,dx', 'f(x)'], ['求和', '\\sum_{i=1}^{n} #', 'x_i'],
    ['向量', '\\vec{#}', 'F'], ['矩阵', '\\begin{bmatrix}# & b \\\\ c & d\\end{bmatrix}', 'a'],
    ['方程组', '\\begin{cases}# = 0 \\\\ y = 1\\end{cases}', 'x+y'],
    ['正应力', '\\sigma = \\frac{#}{A}', 'F'],
    ['弯曲应力', '\\sigma_x = \\frac{# y}{I_z}', 'M_z'],
    ['梁挠曲', 'EI\\frac{d^4 #}{dx^4} = q(x)', 'w'],
    ['希腊字母', '\\alpha + \\beta + \\theta + \\sigma + \\varepsilon', ''],
  ];
  const normalize = value => {
    let text = value.trim();
    for (const [start, end] of [['$$', '$$'], ['\\[', '\\]'], ['\\(', '\\)'], ['$', '$']]) {
      if (text.startsWith(start) && text.endsWith(end) && text.length >= start.length + end.length) {
        return text.slice(start.length, -end.length).trim();
      }
    }
    return text;
  };
  let history = [], position = -1, valid = false;
  try { const draft = sessionStorage.getItem(storageKey); if (draft !== null && draft.length <= 12000) source.value = draft; } catch {}
  function render(record = true) {
    if (record && history[position] !== source.value) {
      history = history.slice(0, position + 1); history.push(source.value);
      if (history.length > 80) history.shift(); position = history.length - 1;
    }
    try { sessionStorage.setItem(storageKey, source.value); } catch {}
    $('undo').disabled = position <= 0; $('redo').disabled = position >= history.length - 1;
    const text = normalize(source.value); valid = false; error.textContent = ''; status.textContent = '';
    $('preview').replaceChildren();
    if (text) {
      try {
        katex.render(text, $('preview'), {displayMode:true, output:'mathml', throwOnError:true, trust:false, strict:'ignore', maxExpand:1000, maxSize:20});
        valid = true;
      } catch { error.textContent = '公式暂时无法预览，请检查括号、命令和矩阵分隔符。'; }
    }
    $('copy').disabled = !valid; $('download').disabled = !valid;
  }
  function insert(body, fallback) {
    const selected = source.value.slice(source.selectionStart, source.selectionEnd);
    const value = body.replace('#', selected || fallback);
    if (source.value.length - selected.length + value.length > 12000) {
      status.textContent = '公式过长，请先精简后再插入。'; return;
    }
    const start = source.selectionStart;
    source.setRangeText(value, start, source.selectionEnd, 'end'); source.focus();
    const slot = body.indexOf('#');
    if (slot >= 0) source.setSelectionRange(start + slot, start + slot + (selected || fallback).length);
    render();
  }
  for (const [label, body, fallback] of templates) {
    const button = document.createElement('button'); button.type = 'button'; button.textContent = label;
    button.addEventListener('click', () => insert(body, fallback)); $('templates').append(button);
  }
  function move(delta) {
    const next = position + delta; if (next < 0 || next >= history.length) return;
    position = next; source.value = history[position]; render(false); source.focus();
  }
  const formatted = () => {
    const text = normalize(source.value);
    return $('format').value === 'block' ? '$$\n' + text + '\n$$' : $('format').value === 'inline' ? '$' + text + '$' : text;
  };
  async function copy() {
    if (!valid) return;
    const text = formatted();
    try { await navigator.clipboard.writeText(text); status.textContent = '已复制，回到 XJCode 粘贴即可。'; }
    catch {
      // Keep the draft and chosen format intact when browser clipboard access is denied.
      const field = document.createElement('textarea'); field.value = text; field.setAttribute('aria-label', '待复制公式');
      field.style.cssText = 'position:fixed;left:8px;bottom:8px;width:calc(100% - 16px);height:80px';
      document.body.append(field); field.focus(); field.select();
      const done = document.execCommand('copy');
      if (done) { field.remove(); source.focus(); status.textContent = '已复制，回到 XJCode 粘贴即可。'; }
      else { status.textContent = '请按 Ctrl+C 复制底部选中的公式，按 Esc 返回。'; field.addEventListener('keydown', e => { if (e.key === 'Escape') {field.remove(); source.focus();} }); }
    }
  }
  source.addEventListener('input', () => render());
  $('undo').addEventListener('click', () => move(-1)); $('redo').addEventListener('click', () => move(1));
  $('clear').addEventListener('click', () => {source.value = ''; render(); source.focus();});
  $('copy').addEventListener('click', copy);
  document.addEventListener('keydown', e => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {e.preventDefault(); copy();}
    if ((e.ctrlKey || e.metaKey) && ['z', 'y'].includes(e.key.toLowerCase())) {
      e.preventDefault(); move(e.shiftKey || e.key.toLowerCase() === 'y' ? 1 : -1);
    }
  });
  $('download').addEventListener('click', () => {
    if (!valid) return;
    const url = URL.createObjectURL(new Blob(['$$\n' + normalize(source.value) + '\n$$\n'], {type:'text/markdown;charset=utf-8'}));
    const a = document.createElement('a'); a.href = url; a.download = 'formula.md'; a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  });
  render();
})();
