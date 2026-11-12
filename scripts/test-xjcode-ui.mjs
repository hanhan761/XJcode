const {chromium} = await import(process.env.XJCODE_PLAYWRIGHT_MODULE || 'playwright');
import fs from 'node:fs/promises';import path from 'node:path';import assert from 'node:assert/strict';import {pathToFileURL,fileURLToPath} from 'node:url';import http from 'node:http';
const repo=path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const work=path.resolve(process.argv[2] || path.join(repo,'.build/ui-acceptance')), reports=[];
await fs.mkdir(work,{recursive:true});
const browser=await chromium.launch({headless:true});
const errors=[];let server;
try {
 const context=await browser.newContext({permissions:['clipboard-read','clipboard-write'],acceptDownloads:true});
 const page=await context.newPage();page.on('pageerror',e=>errors.push(String(e)));
 const external=[];page.on('request',r=>{if(/^https?:/.test(r.url()))external.push(r.url())});
 await context.setOffline(true);
 await page.goto(pathToFileURL(repo+'/xjcode-native/assets/formula.html').href);
 await page.waitForSelector('math');
 assert.equal(await page.locator('#templates button').count(),13);
 for(const button of await page.locator('#templates button').all()) {
  await page.locator('#clear').click();await button.click();
  assert.equal(await page.locator('#error').innerText(),'');assert.equal(await page.locator('math').count(),1);
 }
 reports.push('13 templates render offline');
 await page.locator('#source').fill('$$\\frac{a}{b}$$');
 await page.locator('#format').selectOption('block');await page.locator('#copy').click();
 assert.equal((await page.evaluate(()=>navigator.clipboard.readText())).replaceAll('\r\n','\n'),'$$\n\\frac{a}{b}\n$$');
 reports.push('wrapped input normalizes and Markdown clipboard matches');
 await page.locator('#source').fill('\\frac{a}');assert.equal(await page.locator('#copy').isDisabled(),true);assert.ok(await page.locator('#error').innerText());
 await page.locator('#source').fill('F');await page.locator('#source').evaluate(e=>{e.focus();e.select()});await page.getByRole('button',{name:'分式',exact:true}).click();
 assert.equal(await page.locator('#source').inputValue(),'\\frac{F}{b}');
 await page.keyboard.press('Control+z');assert.equal(await page.locator('#source').inputValue(),'F');
 await page.keyboard.press('Control+Shift+z');assert.equal(await page.locator('#source').inputValue(),'\\frac{F}{b}');
 reports.push('selection insertion, undo and redo preserve draft');
 await page.reload();assert.equal(await page.locator('#source').inputValue(),'\\frac{F}{b}');
 await page.locator('#format').selectOption('inline');
 await page.evaluate(()=>Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText:async()=>{throw Error('denied')}}}));
 await page.locator('#copy').click();assert.ok((await page.locator('#status').innerText()).includes('复制'));assert.equal(await page.locator('#source').inputValue(),'\\frac{F}{b}');
 const download=page.waitForEvent('download');await page.locator('#download').click();const dl=await download;await dl.saveAs(work+'/formula.md');assert.equal(await fs.readFile(work+'/formula.md','utf8'),'$$\n\\frac{F}{b}\n$$\n');
 reports.push('reload draft, denied clipboard fallback and Markdown export');
 for(const width of [1440,390,320]) {await page.setViewportSize({width,height:900});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);await page.screenshot({path:work+'/formula-'+width+'.png',fullPage:true});}
 assert.deepEqual(external,[]);reports.push('1440/390/320 layouts; zero network requests');await context.close();
 const site=repo+'/site';
 server=http.createServer(async(req,res)=>{try{const url=new URL(req.url,'http://localhost');const file=path.join(site,url.pathname==='/'?'index.html':url.pathname);res.setHeader('Content-Type',file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':'text/html;charset=utf-8');res.end(await fs.readFile(file));}catch{res.writeHead(404).end()}});
 await new Promise(r=>server.listen(0,'127.0.0.1',r));const origin='http://127.0.0.1:'+server.address().port;
 const c=await browser.newContext({permissions:['clipboard-read','clipboard-write']});const p=await c.newPage();p.on('pageerror',e=>errors.push(String(e)));
 for(const [state,expected] of [['available','模型调用通过'],['upstream_unavailable','上游不可用'],['unknown','待确认'],['unexpected','待确认'],[null,'待确认']]) {
  await p.route('**/shared-model-status.json*',r=>r.fulfill({json:{checked_at:new Date().toISOString(),models:{fixture:{state}}}}));
  await p.goto(origin);await p.waitForFunction(s=>document.getElementById('shared-model-status').textContent.includes(s),expected);await p.unroute('**/shared-model-status.json*');
 }
 reports.push('known, unknown and malformed health states');
 await p.locator('#formula summary').click();assert.equal(await p.locator('#formula-command').innerText(),'xjcode formula');await p.locator('[data-copy="formula-command"]').click();assert.equal(await p.evaluate(()=>navigator.clipboard.readText()),'xjcode formula');
 for(const width of [1440,390,320]) {await p.setViewportSize({width,height:900});assert.equal(await p.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);await p.screenshot({path:work+'/site-'+width+'.png',fullPage:true});}
 assert.deepEqual(errors,[]);reports.push('site formula command and responsive layouts; no JS errors');
 await fs.writeFile(work+'/browser-results.json',JSON.stringify({success:true,checks:reports},null,2));console.log(JSON.stringify({success:true,checks:reports}));
}finally{await browser.close();if(server)await new Promise(r=>server.close(r))}
