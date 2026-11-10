// Actual generated WASM + real egui canvas. No simulated editor or private notes.
// PLAYWRIGHT_MODULE=/path/to/playwright CHROME=/path/to/chrome node scripts/test-web.mjs
import {createRequire} from 'node:module';
import {createServer} from 'node:http';
import {readFile, mkdir, writeFile} from 'node:fs/promises';
import {resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
const require = createRequire(import.meta.url);
const {chromium,firefox} = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browserName=process.env.BROWSER || 'chromium';
assert(['chromium','firefox'].includes(browserName));
const root = process.env.WEB_ROOT || resolve(dirname(fileURLToPath(import.meta.url)), '..', 'target/web');
const evidence = process.env.EVIDENCE_DIR || resolve(root, 'qa');
await mkdir(evidence, {recursive:true});
const server = createServer(async (req,res) => {
  const pathname = new URL(req.url,'http://localhost').pathname;
  const files = {'/demo/browser-store.js':['browser-store.js','text/javascript'], '/demo/': ['index.html','text/html'], '/demo/index.html':['index.html','text/html'], '/demo/velocimd.js':['velocimd.js','text/javascript'], '/demo/velocimd_bg.wasm':['velocimd_bg.wasm','application/wasm']};
  const file = files[pathname];
  if (!file) { res.writeHead(404);res.end();return; }
  try { res.writeHead(200,{'Content-Type':file[1], 'Cache-Control':'no-store'});res.end(await readFile(resolve(root,file[0]))); }
  catch { res.writeHead(500);res.end(); }
});
await new Promise(r => server.listen(0,'127.0.0.1',r));
const url = `http://127.0.0.1:${server.address().port}/demo/index.html`;
let browser;
const errors=[];
const results = [];
async function test(name, fn) { await fn(); results.push({name,status:'passed'});console.log('PASS',name); }
async function ready(page) { await page.goto(url); await page.waitForFunction(() => document.documentElement.dataset.ready === 'true',null,{timeout:30000}); }
const state = page => page.evaluate(() => JSON.parse(window.velocimd.snapshot()));
const active = page => page.evaluate(() => JSON.parse(window.velocimd.active_document()));
const key = 'velocimd.browser-drafts.v1';
async function persisted(page) {
  return page.evaluate(async () => {
    const db=await new Promise((resolve,reject)=>{const r=indexedDB.open('velocimd.browser-drafts.v2',1);r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);});
    try{return await new Promise((resolve,reject)=>{const tx=db.transaction(['session','documents']);let head,docs;tx.objectStore('session').get('head').onsuccess=e=>head=e.target.result;tx.objectStore('documents').getAll().onsuccess=e=>docs=e.target.result;tx.oncomplete=()=>resolve(JSON.stringify({head,docs}));tx.onabort=()=>reject(tx.error);});}finally{db.close();}
  });
}
async function probe(page) {
  await page.evaluate(() => window.velocimd.set_ui_probe(true));
  await page.waitForFunction(() => JSON.parse(window.velocimd.ui_snapshot()).controls?.length>0);
  return page.evaluate(() => JSON.parse(window.velocimd.ui_snapshot()));
}
async function click(page,id) {
  const ui=await probe(page);
  const control=ui.controls.find(c=>c.id===id);
  assert(control,`missing real widget ${id}`);assert(control.enabled,`disabled widget ${id}`);
  const [x,y,w,h]=control.rect;
  await page.mouse.click(x+w/2,y+h/2);
  await page.evaluate(() => new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
}
async function clickTab(page,index) { await click(page,`tab:${index}`);await page.keyboard.press('Escape'); }
async function saveDraft(page) {
  if (await page.locator('#retry').isVisible()) await page.click('#retry');
  else await click(page,'toolbar:save');
  await page.waitForFunction(()=>{const s=JSON.parse(window.velocimd.snapshot());return !s.persistence.inflight && (!s.pending || !!s.error);});
}
try {
  const type=browserName==='firefox'?firefox:chromium;
  browser = await type.launch({...(process.env.CHROME?{executablePath:process.env.CHROME}:{}),headless:process.env.HEADED!=='1',...(browserName==='chromium'?{chromiumSandbox:true}:{})});
  const context = await browser.newContext({viewport:{width:1280,height:800},acceptDownloads:true});
  const page = await context.newPage();

  page.on('pageerror', e => errors.push(String(e)));
  page.on('console', m => { if(m.type()==='error') errors.push(m.text()); });
  await test('startup, subdirectory assets and actual egui canvas', async () => {
    await ready(page);
    assert.equal((await state(page)).documents.length,1);
    assert((await page.locator('canvas').boundingBox()).height === 800);
    assert.deepEqual(await page.evaluate(()=>JSON.parse(window.velocimd.ui_snapshot())),{});
    assert.equal(await page.locator('nav').count(),0);
    assert.equal(await page.locator('#notice').isVisible(),false);
    const measured=await probe(page);
    assert.equal(measured.controls.filter(c=>c.id.startsWith('toolbar:')).length,11);
    assert.equal(measured.controls.find(c=>c.id==='toolbar:open-folder').enabled,false);
    assert.equal(measured.regions.top_bar[3],42);
    assert.equal(measured.regions.command_bar[3],38);
    await writeFile(resolve(evidence,'startup-ui.json'),JSON.stringify(measured,null,2));
    await page.screenshot({path:resolve(evidence,'startup.png')});
  });
  await test('new document, real keyboard editing, preview and theme', async () => {
    await click(page,'toolbar:new');
    assert.equal((await state(page)).documents.length,2);
    await click(page,'toolbar:edit');
    const ui=await probe(page);
    const r=ui.regions.editor;
    await page.mouse.click(r[0]+90,r[1]+25);
    await page.keyboard.press('Control+Home');
    await page.keyboard.press('Control+a');
    await page.keyboard.insertText('# Browser QA');
    await page.keyboard.press('Enter');
    await page.keyboard.press('Enter');
    await page.keyboard.insertText('Actual egui edit — café 🦀');
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => JSON.parse(window.velocimd.active_document()).content === '# Browser QA\n\nActual egui edit — café 🦀\n');
    assert.equal((await active(page)).content,'# Browser QA\n\nActual egui edit — café 🦀\n');
    // Prove debounce reaches a completed real IndexedDB transaction without Retry.
    await page.waitForFunction(() => !JSON.parse(window.velocimd.snapshot()).pending);
    assert((await persisted(page)).includes('Actual egui edit'));
    assert.equal((await state(page)).pending,false);
    await click(page,'toolbar:preview');
    assert.equal((await state(page)).mode,'Preview');
    await page.screenshot({path:resolve(evidence,'preview.png')});
    await click(page,'toolbar:theme');
    assert.equal((await state(page)).light,true);
    await page.screenshot({path:resolve(evidence,'light.png')});
    await click(page,'toolbar:split');
  });
  await test('shared filename editor and real keyboard command palette',async () => {
    await click(page,'filename');
    await page.keyboard.press('Control+a');await page.keyboard.insertText('Browser QA');
    await click(page,'toolbar:preview');
    await page.waitForFunction(()=>JSON.parse(window.velocimd.snapshot()).documents.includes('Browser QA.md'));
    await page.keyboard.press('Control+k');
    await page.waitForFunction(()=>JSON.parse(window.velocimd.snapshot()).palette_open);
    await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
    await page.keyboard.insertText('Switch to edit mode');
    await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
    await page.keyboard.press('Enter');
    await page.waitForFunction(()=>JSON.parse(window.velocimd.snapshot()).mode==='Edit');
    assert((await active(page)).content.includes('Actual egui edit'));
  });
  await test('Unicode selection undo/redo has one egui owner and no native hidden-input echo',async()=>{
    const isolated=await browser.newContext({viewport:{width:1280,height:800}});
    const p=await isolated.newPage();p.on('pageerror',e=>errors.push(String(e)));
    try {
      await ready(p);
      const original='alpha e\u0301 🦀\r\n';const edited='猫'+original.slice(1);
      await p.setInputFiles('#file',{name:'undo.md',mimeType:'text/markdown',buffer:Buffer.from(original)});
      await p.waitForFunction(s=>JSON.parse(velocimd.active_document()).content===s,original);
      await click(p,'toolbar:edit');const r=(await probe(p)).regions.editor;
      await p.mouse.click(r[0]+70,r[1]+30);await p.keyboard.press('Control+Home');await p.keyboard.press('Shift+ArrowRight');
      await p.waitForTimeout(1200);
      await p.keyboard.insertText('猫');await p.waitForFunction(s=>JSON.parse(velocimd.active_document()).content===s,edited);
      await p.keyboard.press('Control+z');await p.waitForFunction(s=>JSON.parse(velocimd.active_document()).content===s,original);
      await p.keyboard.press('Control+Shift+z');await p.waitForFunction(s=>JSON.parse(velocimd.active_document()).content===s,edited);
      const guards=await p.evaluate(()=>{
        const agent=document.activeElement;if(!(agent instanceof HTMLInputElement)||agent.id)throw Error('Expected the actual focused eframe text agent');
        const prevented=(target,inputType,isComposing=false)=>{const e=new InputEvent('beforeinput',{bubbles:true,cancelable:true,inputType,isComposing});target.dispatchEvent(e);return e.defaultPrevented;};
        return [prevented(agent,'historyUndo'),prevented(agent,'historyRedo'),prevented(agent,'insertText'),prevented(agent,'insertCompositionText',true),prevented(agent,'historyUndo',true),prevented(document.getElementById('download-name'),'historyUndo')];
      });
      assert.deepEqual(guards,[true,true,false,false,false,false]);
      await p.evaluate(()=>velocimd.action('flush',0));await p.waitForFunction(()=>!JSON.parse(velocimd.snapshot()).pending);
      assert.equal((await active(p)).content,edited);
    }finally{await isolated.close();await page.bringToFront();}
  });
  await test('incompatible text-agent assembly releases its writer and preserves migrated bytes',async()=>{
    const isolated=await browser.newContext({viewport:{width:1280,height:800}});
    const p=await isolated.newPage();
    const content='retained adapter recovery 🦀\r\n';
    const raw=JSON.stringify({version:1,documents:[{title:'guard.md',content}],active:0,mode:'Edit',split_ratio:0.5,light:false});
    try {
      await p.addInitScript(raw=>{
        localStorage.setItem('velocimd.browser-drafts.v1',raw);
        const observer=new MutationObserver(records=>{
          if(records.some(r=>[...r.addedNodes].some(n=>n instanceof HTMLInputElement&&n.type==='text'&&n.autofocus&&!n.id&&n.parentElement===document.body))){
            observer.disconnect();const extra=document.createElement('input');extra.type='text';extra.autofocus=true;document.body.append(extra);
          }
        });observer.observe(document,{childList:true,subtree:true});
      },raw);
      await p.goto(url);await p.waitForFunction(()=>document.getElementById('error').textContent.includes('text-input adapter changed'));
      assert.notEqual(await p.evaluate(()=>document.documentElement.dataset.ready),'true');
      await p.waitForFunction(async()=>!(await navigator.locks.query()).held.some(l=>l.name==='velocimd.browser-drafts.v1.writer'));
      const peer=await isolated.newPage();await ready(peer);assert.equal((await active(peer)).content,content);assert.equal((await state(peer)).error,null);
    }finally{await isolated.close();await page.bringToFront();}
  });
  const imported = Buffer.from('\ufeff# Imported\r\n\r\ncafé 🦀\r\nNo final newline');
  await test('import and exact active UTF-8/CRLF/BOM download', async () => {
    const chooser=page.waitForEvent('filechooser');
    await click(page,'toolbar:open-file');
    await (await chooser).setFiles({name:'exact.MD',mimeType:'text/markdown',buffer:imported});
    await page.waitForFunction(() => JSON.parse(window.velocimd.active_document()).title === 'exact.MD');
    assert.deepEqual(Buffer.from((await active(page)).content), imported);
    const downloadP = page.waitForEvent('download');
    await click(page,'toolbar:save');
    const download = await downloadP;
    assert.equal(download.suggestedFilename(),'exact.MD');
    assert.deepEqual(await readFile(await download.path()),imported);
  });
  await test('Save As chooses a different downloaded filename and can cancel safely',async () => {
    await click(page,'toolbar:save-as');
    await page.locator('#save-as').waitFor({state:'visible'});
    await page.fill('#download-name','renamed-copy.md');
    const dp=page.waitForEvent('download');await page.click('#save-copy');
    const copy=await dp;assert.equal(copy.suggestedFilename(),'renamed-copy.md');
    assert.deepEqual(await readFile(await copy.path()),imported);
    assert.equal((await active(page)).title,'exact.MD','download name does not pretend to rename the imported disk file');
    await click(page,'toolbar:save-as');await page.locator('#save-as').waitFor({state:'visible'});
    await page.click('#cancel-copy');assert.deepEqual(Buffer.from((await active(page)).content),imported);
    await click(page,'toolbar:split');
  });
  await test('reload restores all tabs, exact active body, mode and theme', async () => {
    await saveDraft(page);
    await page.waitForFunction(() => !JSON.parse(window.velocimd.snapshot()).pending);
    const before = await state(page);
    const stored = await persisted(page);
    assert(stored.includes('Actual egui edit'));
    await page.reload();
    await page.waitForFunction(() => document.documentElement.dataset.ready==='true');
    const after = await state(page);
    assert.deepEqual(after.documents,before.documents);
    assert.equal(after.active,before.active);
    assert.equal(after.mode,'Split');
    assert.equal(after.light,true);
    assert.deepEqual(Buffer.from((await active(page)).content),imported);
    await clickTab(page,1);
    assert((await active(page)).content.includes('Actual egui edit'));
    await clickTab(page,2);
  });
  await test('retained-page teardown drains a pending new tab before releasing its writer lease', async () => {
    await click(page,'toolbar:new');
    const before = await state(page);
    assert(before.pending);
    await page.evaluate(()=>window.dispatchEvent(new PageTransitionEvent('pagehide',{persisted:true})));
    await page.waitForFunction(async()=>!(await navigator.locks.query()).held.some(l=>l.name==='velocimd.browser-drafts.v1.writer'));
    await page.reload();
    await page.waitForFunction(() => document.documentElement.dataset.ready === 'true');
    assert.deepEqual((await state(page)).documents,before.documents);
    assert.equal((await state(page)).active,before.active);
    await clickTab(page,2);
  });
  await test('close active browser tab selects remaining document without deleting other notes', async () => {
    const before = await state(page);
    page.once('dialog',d=>d.dismiss());
    await click(page,'toolbar:close');
    assert.deepEqual((await state(page)).documents,before.documents,'cancel keeps every open browser draft');
    page.once('dialog',d=>d.accept());
    await click(page,'toolbar:close');
    const after = await state(page);
    assert.equal(after.documents.length,before.documents.length-1);
    assert(!after.documents.includes('exact.MD'));
    await clickTab(page,1);
    assert((await active(page)).content.includes('Actual egui edit'));
  });
  await test('invalid extension, UTF-8 and size leave active content unchanged', async () => {
    const before = await active(page);
    for (const [name,buffer,error] of [['wrong.txt',Buffer.from('wrong'),'.md'],['bad.md',Buffer.from([255]),'UTF-8'],['large.md',Buffer.alloc(1024*1024+1,97),'1 MiB']]) {
      await page.setInputFiles('#file',{name,mimeType:'text/plain',buffer});
      await page.waitForFunction(needle => document.getElementById('error').textContent.includes(needle),error);
      assert.deepEqual(await active(page),before);
    }
  });
  await test('real async file read cannot overwrite newer selection or a new tab', async () => {
    // Delay the actual File.arrayBuffer promise, not an invented completion.
    await page.evaluate(() => {
      window.originalArrayBuffer = File.prototype.arrayBuffer;
      File.prototype.arrayBuffer = function() {
        return new Promise((resolve,reject) => { window.finishRead = () => window.originalArrayBuffer.call(this).then(resolve,reject); });
      };
    });
    await page.setInputFiles('#file',{name:'delayed.md',mimeType:'text/markdown',buffer:Buffer.from('must not arrive')});
    await page.waitForFunction(() => typeof window.finishRead === 'function');
    await click(page,'toolbar:new');
    const before = await active(page);
    await page.evaluate(() => { window.finishRead(); File.prototype.arrayBuffer = window.originalArrayBuffer; });
    await page.waitForFunction(() => document.getElementById('error').textContent.includes('Import cancelled'));
    assert.deepEqual(await active(page),before);
    assert(!(await state(page)).documents.includes('delayed.md'));
  });
  await test('unchanged renderer helper export returns real sanitized HTML', async () => {
    const html = await page.evaluate(async () => {const m=await import('./velocimd.js');return m.render_markdown('# Hello\n\n**bold** <script>alert(1)</script>');});
    assert(html.includes('<h1>Hello</h1>'));
    assert(html.includes('<strong>bold</strong>'));
    assert(!html.includes('<script>'));
  });
  await test('quota failure is visible; exact current content stays downloadable and retry recovers', async () => {
    await saveDraft(page);
    const storedBefore = await persisted(page);
    await page.evaluate(() => {window.savedPut=IDBObjectStore.prototype.put; IDBObjectStore.prototype.put=function(){throw new DOMException('quota fixture','QuotaExceededError');};});
    await click(page,'toolbar:new');
    const before=await active(page);
    await saveDraft(page);
    await page.waitForFunction(() => document.getElementById('error').textContent.includes('quota'));
    assert((await state(page)).pending);
    assert.equal(await persisted(page),storedBefore);
    const dp=page.waitForEvent('download'); await click(page,'toolbar:save');
    assert.equal((await readFile(await (await dp).path())).toString(),before.content);
    await page.screenshot({path:resolve(evidence,'quota-error.png')});
    await page.evaluate(() => {IDBObjectStore.prototype.put=window.savedPut;});
    await saveDraft(page);
    assert.equal((await state(page)).pending,false);
    assert.equal((await state(page)).error,null);
  });
  await test('cross-tab conflict preserves persisted bytes and in-memory edits', async () => {
    await page.evaluate(k=>localStorage.setItem(k,'other-tab-fixture'),key);
    await click(page,'toolbar:new');
    const before=await active(page);
    await saveDraft(page);
    await page.waitForFunction(() => document.getElementById('error').textContent.includes('another browser tab'));
    assert.equal(await page.evaluate(k=>localStorage.getItem(k),key),'other-tab-fixture');
    assert.deepEqual(await active(page),before);
  });
  assert.deepEqual(errors,[], 'no browser panic or console error');
  await context.close();
  await test('malformed persisted bytes are recoverable and never silently overwritten', async () => {
    const ctx=await browser.newContext(); const p=await ctx.newPage();
    await p.addInitScript(k=>{ if(!sessionStorage.getItem('seeded')){localStorage.setItem(k,'{corrupt fixture');sessionStorage.setItem('seeded','1');} },key);
    await ready(p);
    await p.waitForFunction(() => document.getElementById('error').textContent.includes('malformed'));
    await click(p,'toolbar:new'); await saveDraft(p);
    assert.equal(await p.evaluate(k=>localStorage.getItem(k),key),'{corrupt fixture');
    const dp=p.waitForEvent('download');await p.click('#recovery');
    assert.equal((await readFile(await (await dp).path())).toString(),'{corrupt fixture');
    p.once('dialog', d=>d.accept());await p.click('#replace');
    await p.waitForFunction(() => JSON.parse(window.velocimd.snapshot()).pending===false);
    assert(await p.evaluate(async()=>{const db=await new Promise(r=>{const q=indexedDB.open('velocimd.browser-drafts.v2');q.onsuccess=()=>r(q.result);});try{return await new Promise(r=>{const q=db.transaction('recovery').objectStore('recovery').getAll();q.onsuccess=()=>r(q.result.some(v=>typeof v==='string' && (v==='{corrupt fixture' || JSON.parse(v).legacy==='{corrupt fixture')));});}finally{db.close();}}));
    await ctx.close();
  });
  await test('storage read denial starts usable editor with visible non-durable warning', async () => {
    const ctx=await browser.newContext();const p=await ctx.newPage();
    await p.addInitScript(() => {
      const storage = window.localStorage;
      window.restoreStorage = () => Object.defineProperty(window,'localStorage',{configurable:true,value:storage});
      Object.defineProperty(window,'localStorage',{configurable:true,get(){throw new DOMException('fixture','SecurityError');}});
    });
    await ready(p);
    assert((await state(p)).error.includes('unavailable'));
    await click(p,'toolbar:new');
    assert.equal((await state(p)).documents.length,2);
    const dp=p.waitForEvent('download');await click(p,'toolbar:save');
    assert.equal((await readFile(await (await dp).path())).toString(),(await active(p)).content);
    await p.screenshot({path:resolve(evidence,'storage-denied.png')});
    await p.evaluate(() => { window.restoreStorage(); });
    await saveDraft(p);
    assert.equal((await state(p)).pending,false);
    assert.equal((await state(p)).error,null);
    await ctx.close();
  });
  await test('origin writer lease blocks a competing editor and releases safely on close', async () => {
    const ctx=await browser.newContext();const first=await ctx.newPage();
    try {
      await ready(first);
      await first.evaluate(() => {
        const h=window.velocimd;
        h.complete_import(h.begin_import(),'lease-A.md',new TextEncoder().encode('Writer A retained\n'));
        h.action('retry',0);
      });
      await first.waitForFunction(()=>!JSON.parse(window.velocimd.snapshot()).pending);
      const popup=first.waitForEvent('popup');
      await first.evaluate(u=>{window.peer=window.open(u);},url);
      const second=await popup;
      await second.waitForFunction(()=>document.documentElement.dataset.ready==='true'||document.getElementById('status').textContent.includes('could not start'));
      const competing=await second.evaluate(()=>document.documentElement.dataset.ready==='true');
      if(competing) {
        // Controlled interleaving of two real app writers, not a hypothetical log.
        const overlap=await first.evaluate(k=>{
          const a=window.velocimd,b=window.peer.velocimd;
          a.complete_import(a.begin_import(),'writer-A-extra.md',new TextEncoder().encode('Writer A newer\n'));
          b.complete_import(b.begin_import(),'writer-B-extra.md',new TextEncoder().encode('Writer B newer\n'));
          const original=Storage.prototype.setItem;let armed=true;
          Storage.prototype.setItem=function(key,value){if(key===k&&armed){armed=false;b.action('retry',0);}return original.call(this,key,value);};
          try {a.action('retry',0);} finally {Storage.prototype.setItem=original;}
          return {a:JSON.parse(a.snapshot()),b:JSON.parse(b.snapshot()),stored:JSON.parse(localStorage.getItem(k)),mechanism:'A passes its baseline read; B writes and acknowledges before A setItem resumes and overwrites B'};
        },key);
        await writeFile(resolve(evidence,'overlap-control.json'),JSON.stringify(overlap,null,2));
      }
      assert.equal(competing,false,'a second editor must not become a concurrent draft writer');
      assert.match(await second.locator('#error').innerText(),/Another Velocimd editor/);
      assert.equal(await second.evaluate(()=>typeof window.velocimd),'undefined');
      await first.evaluate(()=>window.dispatchEvent(new PageTransitionEvent('pagehide',{persisted:true})));
      assert.equal(await first.evaluate(()=>typeof window.velocimd),'undefined');
      await second.reload();
      await second.waitForFunction(()=>document.documentElement.dataset.ready==='true');
      assert.equal((await active(second)).content,'Writer A retained\n');
      // The controlled retained-page signal above exercises the actual teardown
      // handler; natural close must also hand the lease to a subsequent editor.
      await second.close();
      await first.reload();
      await first.waitForFunction(()=>document.documentElement.dataset.ready==='true');
      assert.equal((await active(first)).content,'Writer A retained\n');
    } finally {await ctx.close();}
  });
  await test('missing Web Locks fails closed instead of admitting an unsafe writer', async () => {
    const ctx=await browser.newContext();const p=await ctx.newPage();
    try {
      await p.addInitScript(()=>Object.defineProperty(navigator,'locks',{configurable:true,value:undefined}));
      await p.goto(url);
      await p.waitForFunction(()=>document.getElementById('error').textContent.includes('Web Locks'));
      assert.equal(await p.evaluate(()=>document.documentElement.dataset.ready==='true'),false);
      assert.equal(await p.evaluate(()=>typeof window.velocimd),'undefined');
    } finally {await ctx.close();}
  });
  await writeFile(resolve(evidence,'results.json'),JSON.stringify({url,browser:browser.version(),sandbox:browserName==='chromium'?'chromiumSandbox:true':'Firefox defaults; no sandbox override',browserName,results,consoleErrors:errors},null,2));
  console.log(JSON.stringify({passed:results.length,failed:0,evidence}));
} catch(error) {
  const diagnostics=[];
  for(const context of browser?.contexts() || [])for(const page of context.pages()) {
    try {
      diagnostics.push(await page.evaluate(()=>({html:document.body.innerText,ready:document.documentElement.dataset.ready,state:window.velocimdState,ui:window.velocimd?.ui_snapshot()})));
      await page.screenshot({path:resolve(evidence,`failure-${diagnostics.length}.png`)});
    }catch{}
  }
  await writeFile(resolve(evidence,'failure.json'),JSON.stringify({results,consoleErrors:errors,diagnostics,error:String(error),stack:error.stack},null,2));
  throw error;
} finally { if(browser)await browser.close();await new Promise(r=>server.close(r)); }
