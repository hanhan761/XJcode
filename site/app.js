const status=document.getElementById('copy-status');let timer;
for(const button of document.querySelectorAll('[data-copy]')){button.addEventListener('click',async()=>{const target=document.getElementById(button.dataset.copy);if(!target)return;const text=target.textContent.trim();try{await navigator.clipboard.writeText(text);status.textContent='已复制';}catch{const range=document.createRange();range.selectNodeContents(target);const selection=window.getSelection();selection.removeAllRanges();selection.addRange(range);status.textContent='请按 Ctrl+C 复制';}status.classList.add('visible');clearTimeout(timer);timer=setTimeout(()=>status.classList.remove('visible'),2000);});}

// This is an aggregate reference, not a verdict on a student's personal account.
async function refreshSharedModelStatus(){
 const target=document.getElementById('shared-model-status');if(!target)return;
 let message='共享账号池状态待确认';
 try{const response=await fetch('/shared-model-status.json?t='+Date.now(),{cache:'no-store'});if(!response.ok)throw Error('unavailable');const report=await response.json();const age=Date.now()-Date.parse(report.checked_at);if(!Number.isFinite(age)||age < -300000||age>28*3600000||!report.models||typeof report.models!=='object')throw Error('stale');const values=Object.values(report.models);if(!values.length)throw Error('empty');const down=values.filter(v=>v?.state==='upstream_unavailable').length;const unknown=values.filter(v=>v?.state==='unknown').length;message=down?`共享账号池检测：${down} 个模型上游不可用`:unknown?`共享账号池检测：${unknown} 个模型状态待确认`:'共享账号池检测：模型调用通过';}catch{}
 target.replaceChildren(document.createTextNode(message+'；个人账号请以实际调用为准。 '));const link=document.createElement('a');link.href='https://www.wenshanzhike.com/model-health-20261111/status.html';link.target='_blank';link.rel='noopener noreferrer';link.textContent='查看状态';target.append(link);
}
refreshSharedModelStatus();setInterval(refreshSharedModelStatus,300000);
