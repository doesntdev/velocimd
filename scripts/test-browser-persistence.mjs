// Issue 11: real WASM/input + real IndexedDB. Fault only browser API boundaries;
// never substitute a fake database or fabricate completion/durability receipts.
import {chromium,firefox} from 'playwright';
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {serveWeb} from './parity/serve.mjs';
const evidence=process.env.EVIDENCE_DIR || resolve('target/web/issue11-qa');
const caseFilter=process.env.CASE_FILTER ? new RegExp(process.env.CASE_FILTER) : null;
await mkdir(evidence,{recursive:true});
const service=await serveWeb(process.env.WEB_ROOT || resolve('target/web'));
const results=[], errors=[];let browser;let stopped=false;
async function cleanup(){if(stopped)return;stopped=true;await browser?.close();await service.close();}
for(const signal of ['SIGINT','SIGTERM'])process.once(signal,()=>{cleanup().finally(()=>process.exit(130));});
const session=(documents=[{title:'Exact.MD',content:'\ufeff# café 🦀\r\nNo final newline'}])=>JSON.stringify({version:1,documents,active:0,mode:'Edit',split_ratio:0.5,light:false},null,1);
const state=p=>p.evaluate(()=>JSON.parse(velocimd.snapshot()));
const active=p=>p.evaluate(()=>JSON.parse(velocimd.active_document()));
async function settled(p){await p.waitForFunction(()=>{const s=JSON.parse(velocimd.snapshot());return !s.persistence.inflight && (!s.pending || s.error);});}
async function save(p){await p.evaluate(()=>velocimd.action('retry',0));await settled(p);}
async function newPage(init,arg) {
  const ctx=await browser.newContext({viewport:{width:1280,height:800},acceptDownloads:true});
  const p=await ctx.newPage();p.on('pageerror',e=>errors.push(String(e)));
  await p.addInitScript(()=>{
    window.dbRun=async(mode,fn)=>{
      const db=await new Promise((resolve,reject)=>{const r=indexedDB.open('velocimd.browser-drafts.v2',1);r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error);});
      try{return await new Promise((resolve,reject)=>{const tx=db.transaction(['session','documents','recovery'],mode);let value;fn(tx,v=>value=v);tx.oncomplete=()=>resolve(value);tx.onabort=()=>reject(tx.error);});}finally{db.close();}
    };
    window.dbRead=()=>dbRun('readonly',(tx,done)=>{
      const out={};for(const [store,key] of [['session','head'],['session','migration']])tx.objectStore(store).get(key).onsuccess=e=>out[key]=e.target.result??null;
      for(const store of ['documents','recovery'])tx.objectStore(store).getAll().onsuccess=e=>out[store]=e.target.result;
      tx.addEventListener('complete',()=>{});done(out);
    });
    window.holdWrites=async()=>{
      const db=await new Promise(r=>{const q=indexedDB.open('velocimd.browser-drafts.v2');q.onsuccess=()=>r(q.result);});
      let release;const held=new Promise(r=>release=r);
      const tx=db.transaction('session','readwrite');let keep=true;window.releaseWrites=()=>{keep=false;};
      const pump=()=>{const q=tx.objectStore('session').get('head');q.onsuccess=()=>{release();if(keep)pump();};};pump();
      tx.oncomplete=()=>db.close();tx.onabort=()=>db.close();await held;
    };
  });
  if(init)await p.addInitScript(init,arg);
  await p.goto(service.url);await p.waitForFunction(()=>document.documentElement.dataset.ready==='true',null,{timeout:30000});
  return {p,ctx};
}
async function edit(p,text) {
  await p.evaluate(()=>{velocimd.action('edit',0);velocimd.set_ui_probe(true);});
  await p.waitForFunction(()=>JSON.parse(velocimd.ui_snapshot()).regions?.editor);
  const r=await p.evaluate(()=>JSON.parse(velocimd.ui_snapshot()).regions.editor);
  await p.mouse.click(r[0]+70,r[1]+30);await p.keyboard.press('Control+Home');if(text!=='')await p.keyboard.insertText(text);
  await p.waitForFunction(t=>JSON.parse(velocimd.active_document()).content.startsWith(t),text);
}
async function setExact(p,text) {
  await edit(p,'');await p.keyboard.press('Control+a');await p.keyboard.insertText(text);
  await p.waitForFunction(t=>JSON.parse(velocimd.active_document()).content===t,text);
}
async function download(p) {
  const dp=p.waitForEvent('download');
  await p.evaluate(()=>velocimd.set_ui_probe(true));
  await p.waitForFunction(()=>JSON.parse(velocimd.ui_snapshot()).controls?.some(c=>c.id==='toolbar:save'));
  const r=await p.evaluate(()=>JSON.parse(velocimd.ui_snapshot()).controls.find(c=>c.id==='toolbar:save').rect);
  await p.mouse.click(r[0]+r[2]/2,r[1]+r[3]/2);
  const d=await dp;return readFile(await d.path());
}
async function test(name,fn) {if(caseFilter && !caseFilter.test(name))return;await fn();results.push({name,status:'passed'});console.log('PASS',name);}
try {
  const type=process.env.BROWSER==='firefox'?firefox:chromium;
  browser=await type.launch({headless:process.env.HEADED!=='1',...(type===chromium?{executablePath:process.env.CHROME||'/usr/bin/google-chrome',chromiumSandbox:true}:{})});
  await test('legacy migration commits, verifies exact raw backup, then installs fail-closed v1 marker',async()=>{
    const raw=session();const {p,ctx}=await newPage(({raw})=>{if(!sessionStorage.seeded){localStorage.setItem('velocimd.browser-drafts.v1',raw);sessionStorage.seeded='1';}},{raw});
    try {
      assert.equal((await active(p)).content,'\ufeff# café 🦀\r\nNo final newline');
      const db=await p.evaluate(()=>dbRead());assert(db.recovery.includes(raw));assert.equal(JSON.parse(db.head).version,2);
      const marker=await p.evaluate(()=>localStorage.getItem('velocimd.browser-drafts.v1'));assert.equal(JSON.parse(marker).version,2);
      assert(await p.evaluate(m=>{try{velocimdValidateDrafts(m);return false;}catch{return true;}},marker),'the old exact Rust v1 decoder refuses the marker');
      await edit(p,'new ');await save(p);assert.equal((await state(p)).pending,false);
      await p.reload();await p.waitForFunction(()=>document.documentElement.dataset.ready==='true');assert((await active(p)).content.startsWith('new '));
    }finally{await ctx.close();}
  });
  await test('legacy transaction abort leaves original raw bytes untouched and editor downloadable',async()=>{
    const raw=session();const {p,ctx}=await newPage(({raw})=>{
      if(!sessionStorage.seeded){localStorage.setItem('velocimd.browser-drafts.v1',raw);sessionStorage.seeded='1';}
      const put=IDBObjectStore.prototype.put;window.restorePut=()=>IDBObjectStore.prototype.put=put;
      IDBObjectStore.prototype.put=function(v,k){const r=put.call(this,v,k);if(this.name==='session' && k==='head')this.transaction.abort();return r;};
    },{raw});
    try {
      assert((await state(p)).error);assert.equal(await p.evaluate(()=>localStorage.getItem('velocimd.browser-drafts.v1')),raw);
      const db=await p.evaluate(()=>dbRead());assert.equal(db.head,null);assert.deepEqual(db.documents,[]);
      await edit(p,'retained ');assert((await download(p)).toString().startsWith('retained '));
      await p.evaluate(()=>restorePut());p.once('dialog',d=>d.accept());await p.click('#replace');await settled(p);
      assert.equal((await state(p)).error,null);assert((await p.evaluate(()=>dbRead())).recovery.some(v=>typeof v==='string' && v.includes('Exact.MD')));
    }finally{await ctx.close();}
  });
  await test('outside legacy mutation during final migration readback cannot be overwritten by the marker',async()=>{
    const raw=session();const {p,ctx}=await newPage(({raw})=>{
      localStorage.setItem('velocimd.browser-drafts.v1',raw);
      const all=IDBObjectStore.prototype.getAll;let reads=0;
      IDBObjectStore.prototype.getAll=function(...a){const r=all.apply(this,a);if(this.name==='documents' && ++reads===2)r.addEventListener('success',()=>localStorage.setItem('velocimd.browser-drafts.v1','outside during migration readback'));return r;};
    },{raw});
    try {
      assert.equal(await p.evaluate(()=>localStorage.getItem('velocimd.browser-drafts.v1')),'outside during migration readback');
      assert((await state(p)).error);assert((await p.evaluate(()=>dbRead())).recovery.includes(raw));
      await edit(p,'in memory ');await save(p);assert((await state(p)).pending);
      assert.equal(await p.evaluate(()=>localStorage.getItem('velocimd.browser-drafts.v1')),'outside during migration readback');
    }finally{await ctx.close();}
  });
  await test('marker write failure cannot advance v2; exact-baseline retry finalizes migration',async()=>{
    const raw=session();const {p,ctx}=await newPage(({raw})=>{
      if(!sessionStorage.seeded){localStorage.setItem('velocimd.browser-drafts.v1',raw);sessionStorage.seeded='1';}
      const set=Storage.prototype.setItem;window.restoreSet=()=>Storage.prototype.setItem=set;
      Storage.prototype.setItem=function(k,v){if(k==='velocimd.browser-drafts.v1')throw new DOMException('marker quota fixture','QuotaExceededError');return set.call(this,k,v);};
    },{raw});
    try {
      assert((await state(p)).error);const before=await p.evaluate(()=>dbRead());assert(before.recovery.includes(raw));
      await edit(p,'waiting ');await save(p);assert((await state(p)).pending);assert.equal((await p.evaluate(()=>dbRead())).head,before.head);
      assert.equal(await p.evaluate(()=>localStorage.getItem('velocimd.browser-drafts.v1')),raw);
      await p.evaluate(()=>restoreSet());await save(p);assert.equal((await state(p)).error,null);assert.equal((await state(p)).pending,false);
      assert.equal(JSON.parse(await p.evaluate(()=>localStorage.getItem('velocimd.browser-drafts.v1'))).version,2);
    }finally{await ctx.close();}
  });
  await test('interrupted prepared migration reload finalizes only the verified original baseline',async()=>{
    const raw=session();const {p,ctx}=await newPage(({raw})=>{
      if(!sessionStorage.seeded){localStorage.setItem('velocimd.browser-drafts.v1',raw);sessionStorage.seeded='1';const set=Storage.prototype.setItem;Storage.prototype.setItem=function(k,v){if(k==='velocimd.browser-drafts.v1')throw new DOMException('interrupted marker fixture','QuotaExceededError');return set.call(this,k,v);};}
    },{raw});
    try {
      assert((await state(p)).error);const before=await p.evaluate(()=>dbRead());
      await p.reload();await p.waitForFunction(()=>document.documentElement.dataset.ready==='true');assert.equal((await state(p)).error,null);assert.equal((await p.evaluate(()=>dbRead())).head,before.head);
      assert.equal((await active(p)).content,'\ufeff# café 🦀\r\nNo final newline');assert.equal(JSON.parse(await p.evaluate(()=>localStorage.getItem('velocimd.browser-drafts.v1'))).version,2);
    }finally{await ctx.close();}
  });
  await test('outside legacy change during interrupted migration is never adopted by Retry or replacement',async()=>{
    const raw=session();const {p,ctx}=await newPage(({raw})=>{
      if(!sessionStorage.seeded){localStorage.setItem('velocimd.browser-drafts.v1',raw);sessionStorage.seeded='1';}const set=Storage.prototype.setItem;
      Storage.prototype.setItem=function(k,v){if(k==='velocimd.browser-drafts.v1')return;return set.call(this,k,v);};window.restoreSet=()=>Storage.prototype.setItem=set;
    },{raw});
    try {
      const before=await p.evaluate(()=>dbRead());await p.evaluate(()=>{restoreSet();localStorage.setItem('velocimd.browser-drafts.v1','outside legacy bytes');});
      await edit(p,'memory ');await save(p);assert((await state(p)).pending);
      p.once('dialog',d=>d.accept());await p.click('#replace');await settled(p);
      assert.equal((await p.evaluate(()=>dbRead())).head,before.head);assert.equal(await p.evaluate(()=>localStorage.getItem('velocimd.browser-drafts.v1')),'outside legacy bytes');
    }finally{await ctx.close();}
  });
  await test('duplicate fields and unsupported legacy never reach a migration commit',async()=>{
    for(const raw of [session().replace('"version": 1','"version": 1, "version": 1'),session().replace('"version": 1','"version": 3')]) {
      const {p,ctx}=await newPage(({raw})=>localStorage.setItem('velocimd.browser-drafts.v1',raw),{raw});
      try{assert((await state(p)).error);assert.equal((await p.evaluate(()=>dbRead())).head,null);assert.equal(await p.evaluate(()=>localStorage.getItem('velocimd.browser-drafts.v1')),raw);}finally{await ctx.close();}
    }
  });
  await test('real transaction abort rolls back changed bodies AND metadata and permits exact-baseline retry',async()=>{
    const {p,ctx}=await newPage();try {
      await edit(p,'acknowledged ');await save(p);const before=await p.evaluate(()=>dbRead());
      await p.evaluate(()=>{const put=IDBObjectStore.prototype.put;window.restorePut=()=>IDBObjectStore.prototype.put=put;IDBObjectStore.prototype.put=function(v,k){const r=put.call(this,v,k);if(this.name==='session'&&k==='head')this.transaction.abort();return r;};});
      await edit(p,'uncommitted ');await save(p);assert((await state(p)).pending);assert.deepEqual(await p.evaluate(()=>dbRead()),before);
      assert((await download(p)).toString().startsWith('uncommitted '));await p.evaluate(()=>restorePut());await save(p);assert.equal((await state(p)).error,null);
    }finally{await ctx.close();}
  });
  await test('readback denial never reports Saved even after commit; retry verifies latest buffer',async()=>{
    const {p,ctx}=await newPage();try {
      await edit(p,'baseline ');await save(p);
      await p.evaluate(()=>{const get=IDBObjectStore.prototype.get;window.restoreGet=()=>IDBObjectStore.prototype.get=get;IDBObjectStore.prototype.get=function(k){if(this.name==='session'&&this.transaction.mode==='readonly'&&k==='head')throw new DOMException('readback denied fixture','SecurityError');return get.call(this,k);};});
      await edit(p,'committed but not acknowledged ');await save(p);assert((await state(p)).pending);assert((await state(p)).error);
      await p.evaluate(()=>restoreGet());await edit(p,'newest ');await save(p);assert.equal((await state(p)).pending,false);
      await p.reload();await p.waitForFunction(()=>document.documentElement.dataset.ready==='true');assert((await active(p)).content.startsWith('newest '));
    }finally{await ctx.close();}
  });
  await test('reverting to acknowledged text after an unverified commit resends the current body before Saved',async()=>{
    const {p,ctx}=await newPage();try {
      await setExact(p,'AAAA');await save(p);
      await p.evaluate(()=>{const get=IDBObjectStore.prototype.get;window.restoreGet=()=>IDBObjectStore.prototype.get=get;IDBObjectStore.prototype.get=function(k){if(this.name==='session'&&this.transaction.mode==='readonly'&&k==='head')throw new DOMException('readback denied fixture','SecurityError');return get.call(this,k);};});
      await setExact(p,'BBBB');await save(p);assert((await state(p)).error);assert((await state(p)).pending);
      await p.evaluate(()=>restoreGet());assert.equal(JSON.parse((await p.evaluate(()=>dbRead())).documents[0]),'BBBB');
      await setExact(p,'AAAA');await save(p);assert.equal((await state(p)).error,null);assert.equal((await state(p)).pending,false);
      assert.equal(JSON.parse((await p.evaluate(()=>dbRead())).documents[0]),'AAAA','Saved must represent the current reverted body, not the unverified commit');
      await p.reload();await p.waitForFunction(()=>document.documentElement.dataset.ready==='true');assert.equal((await active(p)).content,'AAAA');
    }finally{await ctx.close();}
  });
  await test('delayed old completion cannot acknowledge newer actual keyboard edits and coalesces latest intent',async()=>{
    const {p,ctx}=await newPage();try {
      await edit(p,'baseline ');await save(p);
      await p.evaluate(()=>{const save=velocimdSaveDraftPatch;window.savedAdmission=save;window.admissions=0;velocimdSaveDraftPatch=(...a)=>{admissions++;const real=save(...a);return new Promise(r=>{window.finishOld=()=>real.then(r);});};});
      await edit(p,'older ');await p.evaluate(()=>velocimd.action('flush',0));await p.waitForFunction(()=>typeof finishOld==='function');
      await edit(p,'newest ');await p.keyboard.insertText('rapid ');await p.keyboard.insertText('edits ');
      assert((await state(p)).pending);assert.equal(await p.evaluate(()=>admissions),1);
      await p.evaluate(()=>{velocimdSaveDraftPatch=savedAdmission;finishOld();});await settled(p);
      const s=await state(p);assert.equal(s.error,null);assert.equal(s.pending,false);assert.equal(s.persistence.intent,s.persistence.acknowledged_intent);
      const content=(await active(p)).content;await p.reload();await p.waitForFunction(()=>document.documentElement.dataset.ready==='true');assert.equal((await active(p)).content,content);
    }finally{await ctx.close();}
  });
  await test('outside store head/body changes refuse Retry and recovery replacement without erasing bytes',async()=>{
    for(const kind of ['head','body']) {
      const {p,ctx}=await newPage();try {
        await edit(p,'ack ');await save(p);
        await p.evaluate(kind=>dbRun('readwrite',(tx)=>{if(kind==='head')tx.objectStore('session').put('outside unsupported head','head');else {const q=tx.objectStore('session').get('head');q.onsuccess=()=>tx.objectStore('documents').put('"outside exact body"',JSON.parse(q.result).documents[0].id);}}),kind);
        const outside=await p.evaluate(()=>dbRead());await edit(p,'memory ');await save(p);assert((await state(p)).error);assert((await state(p)).pending);
        p.once('dialog',d=>d.accept());await p.click('#replace');await settled(p);assert.deepEqual(await p.evaluate(()=>dbRead()),outside);
        assert((await download(p)).toString().startsWith('memory '));
      }finally{await ctx.close();}
    }
  });
  await test('real cross-page legacy storage event invalidates an idle acknowledged editor',async()=>{
    const {p,ctx}=await newPage();try {
      await edit(p,'idle acknowledged ');await save(p);const before=await p.evaluate(()=>dbRead());
      const peer=await ctx.newPage();await peer.goto(service.url);await peer.waitForFunction(()=>document.getElementById('error').textContent.includes('Another Velocimd editor'));
      await peer.evaluate(()=>localStorage.setItem('velocimd.browser-drafts.v1','outside idle change'));
      // Firefox pauses a background canvas's RAF; inspect its visible UI only
      // after returning to that tab. The real storage event remains the trigger.
      await p.bringToFront();await p.waitForFunction(()=>!document.hidden);
      await p.waitForFunction(()=>JSON.parse(velocimd.snapshot()).error?.includes('changed'));assert((await state(p)).pending);assert.deepEqual(await p.evaluate(()=>dbRead()),before);
    }finally{await ctx.close();}
  });
  await test('corrupt v2 recovery replacement verifies backup and refuses a later changed baseline',async()=>{
    const {p,ctx}=await newPage();try {
      await edit(p,'recover me ');await save(p);await p.evaluate(()=>dbRun('readwrite',tx=>tx.objectStore('session').put('{corrupt v2 head','head')));
      await p.reload();await p.waitForFunction(()=>document.documentElement.dataset.ready==='true');assert((await state(p)).recovery);
      const raw=await p.evaluate(()=>velocimd.recovery_drafts());assert(raw.includes('{corrupt v2 head'));
      await edit(p,'replacement ');p.once('dialog',d=>d.accept());await p.click('#replace');await settled(p);assert.equal((await state(p)).error,null);
      const db=await p.evaluate(()=>dbRead());assert(db.recovery.some(v=>typeof v==='string'&&v.includes('{corrupt v2 head')));
      await p.reload();await p.waitForFunction(()=>document.documentElement.dataset.ready==='true');assert((await active(p)).content.startsWith('replacement '));
    }finally{await ctx.close();}
  });
  await test('explicit embedded/pagehide teardown holds the shared lease until admitted latest edits drain',async()=>{
    const {p,ctx}=await newPage();try {
      await edit(p,'ack ');await save(p);await p.evaluate(()=>holdWrites());await edit(p,'latest ');await p.evaluate(()=>velocimd.action('flush',0));await edit(p,'final ');
      const expected=(await active(p)).content;await p.evaluate(()=>window.dispatchEvent(new PageTransitionEvent('pagehide',{persisted:true})));
      const peer=await ctx.newPage();await peer.goto(service.url);await peer.waitForFunction(()=>document.getElementById('error').textContent.includes('Another Velocimd editor'));
      assert.equal(await p.evaluate(async()=>(await navigator.locks.query()).held.filter(l=>l.name==='velocimd.browser-drafts.v1.writer').length),1);
      await p.evaluate(()=>releaseWrites());await p.waitForFunction(async()=>!(await navigator.locks.query()).held.some(l=>l.name==='velocimd.browser-drafts.v1.writer'));
      await peer.reload();await peer.waitForFunction(()=>document.documentElement.dataset.ready==='true');assert.equal((await active(peer)).content,expected);
    }finally{await ctx.close();}
  });
  await test('reverting to acknowledged text during teardown preserves the latest body in the successor',async()=>{
    const {p,ctx}=await newPage();try {
      await setExact(p,'AAAA');await save(p);await p.evaluate(()=>holdWrites());
      await setExact(p,'BBBB');await p.evaluate(()=>velocimd.action('flush',0));assert((await state(p)).persistence.inflight);
      await setExact(p,'AAAA');await p.evaluate(()=>window.dispatchEvent(new PageTransitionEvent('pagehide',{persisted:true})));
      await p.evaluate(()=>releaseWrites());
      await p.waitForFunction(async()=>!(await navigator.locks.query()).held.some(l=>l.name==='velocimd.browser-drafts.v1.writer'),null,{polling:50});
      const peer=await ctx.newPage();await peer.goto(service.url);await peer.waitForFunction(()=>document.documentElement.dataset.ready==='true');
      assert.equal((await active(peer)).content,'AAAA','the final admitted reverted body must survive the older write');
      assert.equal(JSON.parse((await p.evaluate(()=>dbRead())).documents[0]),'AAAA');
    }finally{await ctx.close();}
  });
  await test('natural interruption restores the prior acknowledged transaction, never a torn pending save',async()=>{
    const {p,ctx}=await newPage();try {
      await edit(p,'acknowledged ');await save(p);const expected=(await active(p)).content;
      await p.evaluate(()=>holdWrites());await edit(p,'unacknowledged ');await p.evaluate(()=>velocimd.action('flush',0));assert((await state(p)).pending);
      await p.close({runBeforeUnload:false});const next=await ctx.newPage();await next.goto(service.url);await next.waitForFunction(()=>document.documentElement.dataset.ready==='true');
      assert.equal((await active(next)).content,expected);
    }finally{await ctx.close();}
  });
  await test('explicit Retry survives an older held failed completion and saves the latest input',async()=>{
    const {p,ctx}=await newPage(()=>{
      if(sessionStorage.retryOpenRestored)return;
      const open=IDBFactory.prototype.open;
      window.restoreOpen=()=>{IDBFactory.prototype.open=open;sessionStorage.retryOpenRestored='1';};
      IDBFactory.prototype.open=function(){throw new DOMException('IndexedDB denied fixture','SecurityError');};
    });
    try {
      await edit(p,'retained ');await settled(p);
      await p.evaluate(()=>{
        const save=velocimdSaveDraftPatch;window.originalAdmission=save;
        velocimdSaveDraftPatch=(...args)=>{
          const actual=save(...args);
          return new Promise(resolve=>actual.then(outcome=>{
            window.olderOutcome=JSON.parse(outcome);
            window.releaseOlder=()=>resolve(outcome);
          }));
        };
        velocimd.action('retry',0);
      });
      await p.waitForFunction(()=>typeof releaseOlder==='function');
      assert.match((await p.evaluate(()=>olderOutcome)).error,/IndexedDB denied fixture/);
      assert.equal((await state(p)).persistence.inflight,true);
      await edit(p,'latest ');
      const expected=(await active(p)).content;
      await p.evaluate(()=>{restoreOpen();velocimdSaveDraftPatch=originalAdmission;velocimd.action('retry',0);});
      assert.equal((await state(p)).persistence.inflight,true,'Retry is issued while the older failed completion is held');
      await p.evaluate(()=>releaseOlder());await settled(p);
      assert.equal((await state(p)).error,null,'the explicit queued Retry must not be discarded by the older failure');
      assert.equal((await state(p)).pending,false);
      assert.equal(JSON.parse((await p.evaluate(()=>dbRead())).documents[0]),expected);
      await p.reload();await p.waitForFunction(()=>document.documentElement.dataset.ready==='true');
      assert.equal((await active(p)).content,expected);
    }finally{await ctx.close();}
  });
  await test('IndexedDB open denial keeps a usable editor; retry accepts only an unchanged empty baseline',async()=>{
    const {p,ctx}=await newPage(()=>{const open=IDBFactory.prototype.open;window.restoreOpen=()=>IDBFactory.prototype.open=open;IDBFactory.prototype.open=function(){throw new DOMException('IndexedDB denied fixture','SecurityError');};});
    try {
      assert((await state(p)).error);await edit(p,'memory-only ');assert((await download(p)).toString().startsWith('memory-only '));
      await p.evaluate(()=>restoreOpen());await save(p);assert.equal((await state(p)).error,null);assert.equal((await state(p)).pending,false);
    }finally{await ctx.close();}
  });
  await test('recovery backup quota/readback failures preserve original corrupt bytes before replacement',async()=>{
    const {p,ctx}=await newPage(()=>localStorage.setItem('velocimd.browser-drafts.v1','{exact corrupt legacy'));
    try {
      await edit(p,'recoverable ');
      await p.evaluate(()=>{const put=IDBObjectStore.prototype.put;window.restorePut=()=>IDBObjectStore.prototype.put=put;IDBObjectStore.prototype.put=function(...a){if(this.name==='recovery')throw new DOMException('backup quota fixture','QuotaExceededError');return put.apply(this,a);};});
      p.once('dialog',d=>d.accept());await p.click('#replace');await settled(p);assert((await state(p)).error);assert.equal((await p.evaluate(()=>dbRead())).head,null);
      await p.evaluate(()=>{restorePut();const get=IDBObjectStore.prototype.get;window.restoreGet=()=>IDBObjectStore.prototype.get=get;IDBObjectStore.prototype.get=function(...a){if(this.name==='recovery')throw new DOMException('backup readback fixture','SecurityError');return get.apply(this,a);};});
      p.once('dialog',d=>d.accept());await p.click('#replace');await settled(p);assert((await state(p)).error);assert.equal((await p.evaluate(()=>dbRead())).head,null);
      assert.equal(await p.evaluate(()=>localStorage.getItem('velocimd.browser-drafts.v1')),'{exact corrupt legacy');
      await p.evaluate(()=>restoreGet());p.once('dialog',d=>d.accept());await p.click('#replace');await settled(p);assert.equal((await state(p)).error,null);
    }finally{await ctx.close();}
  });
  await test('confirmed recovery replacement coalesces behind an older in-flight failed save',async()=>{
    const {p,ctx}=await newPage(()=>localStorage.setItem('velocimd.browser-drafts.v1','{recovery queue original'));
    try {
      await edit(p,'latest replacement ');await settled(p);
      await p.evaluate(()=>{const save=velocimdSaveDraftPatch;window.savedAdmission=save;velocimdSaveDraftPatch=(...a)=>{const actual=save(...a);return new Promise(resolve=>window.finishOlder=()=>actual.then(resolve));};velocimd.action('retry',0);velocimd.action('replace-recovery',0);});
      assert((await state(p)).pending);await p.evaluate(()=>{velocimdSaveDraftPatch=savedAdmission;finishOlder();});await settled(p);
      assert.equal((await state(p)).error,null);assert.equal((await state(p)).pending,false);
      assert((await p.evaluate(()=>dbRead())).recovery.some(v=>typeof v==='string'&&v.includes('{recovery queue original')));
    }finally{await ctx.close();}
  });
  await test('recovery replacement refuses a store changed after corrupt startup',async()=>{
    const {p,ctx}=await newPage(()=>localStorage.setItem('velocimd.browser-drafts.v1','{original corrupt legacy'));
    try {
      await p.evaluate(()=>dbRun('readwrite',tx=>tx.objectStore('session').put('{outside newer head','head')));
      const before=await p.evaluate(()=>dbRead());p.once('dialog',d=>d.accept());await p.click('#replace');await settled(p);
      assert((await state(p)).error);assert.deepEqual(await p.evaluate(()=>dbRead()),before);
    }finally{await ctx.close();}
  });
  await test('actual same-origin iframe handoff preserves drafts and the shared full-page writer lock',async()=>{
    const {p,ctx}=await newPage();try {
      await edit(p,'full-page acknowledged ');await save(p);const expected=(await active(p)).content;
      await p.evaluate(()=>window.dispatchEvent(new PageTransitionEvent('pagehide',{persisted:true})));
      await p.waitForFunction(async()=>!(await navigator.locks.query()).held.some(l=>l.name==='velocimd.browser-drafts.v1.writer'));
      await p.evaluate(url=>{const frame=document.createElement('iframe');frame.id='embed';frame.src=url;frame.style='position:fixed;inset:0;width:100%;height:100%';document.body.append(frame);},service.url);
      const frame=await (await p.waitForSelector('#embed')).contentFrame();await frame.waitForFunction(()=>document.documentElement.dataset.ready==='true');assert.equal((await active(frame)).content,expected);
      const peer=await ctx.newPage();await peer.goto(service.url);await peer.waitForFunction(()=>document.getElementById('error').textContent.includes('Another Velocimd editor'));
      await frame.evaluate(()=>{velocimd.action('theme',0);window.dispatchEvent(new PageTransitionEvent('pagehide',{persisted:true}));});
      await p.waitForFunction(async()=>!(await navigator.locks.query()).held.some(l=>l.name==='velocimd.browser-drafts.v1.writer'));
      await p.evaluate(()=>document.getElementById('embed').remove());await peer.reload();await peer.waitForFunction(()=>document.documentElement.dataset.ready==='true');assert.equal((await active(peer)).content,expected);assert.equal((await state(peer)).light,true);
    }finally{await ctx.close();}
  });
  await test('browser import completion remains single-use and cannot replay into a newer tab',async()=>{
    const {p,ctx}=await newPage();try {
      const result=await p.evaluate(()=>{const h=velocimd;const generation=h.begin_import();h.complete_import(generation,'once.md',new TextEncoder().encode('exact replay bytes\r\n'));const before=h.active_document();let error;try{h.complete_import(generation,'replayed.md',new TextEncoder().encode('must not arrive'));}catch(e){error=String(e);}return {before,after:h.active_document(),error,documents:JSON.parse(h.snapshot()).documents};});
      assert.match(result.error,/Import cancelled/);assert.equal(result.after,result.before);assert(!result.documents.includes('replayed.md'));await save(p);
    }finally{await ctx.close();}
  });
  await test('valid-checksum v2 data rejected by Rust Unicode validation remains blocked through Retry',async()=>{
    const {p,ctx}=await newPage();try {
      await edit(p,'valid before corruption ');await save(p);
      await p.evaluate(async()=>{
        const body='"\\ud800"';const digest=[...new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(body)))].map(b=>b.toString(16).padStart(2,'0')).join('');
        await dbRun('readwrite',tx=>{const r=tx.objectStore('session').get('head');r.onsuccess=()=>{const h=JSON.parse(r.result);h.documents[0].body_bytes=new TextEncoder().encode(body).length;h.documents[0].checksum=digest;tx.objectStore('documents').put(body,h.documents[0].id);tx.objectStore('session').put(JSON.stringify(h),'head');};});
      });
      await p.reload();await p.waitForFunction(()=>document.documentElement.dataset.ready==='true');assert((await state(p)).error);
      const before=await p.evaluate(()=>dbRead());await edit(p,'safe in-memory ');await save(p);assert((await state(p)).error);assert((await state(p)).pending);assert.deepEqual(await p.evaluate(()=>dbRead()),before);
    }finally{await ctx.close();}
  });
  await test('untouched outside-body corruption survives metadata writes and is rejected by startup checksum validation',async()=>{
    const {p,ctx}=await newPage();try {
      await edit(p,'active intact ');await p.evaluate(()=>{const h=velocimd;h.complete_import(h.begin_import(),'other.md',new TextEncoder().encode('outside target'));h.action('select',0);});await save(p);
      await p.evaluate(()=>dbRun('readwrite',tx=>{const r=tx.objectStore('session').get('head');r.onsuccess=()=>tx.objectStore('documents').put(JSON.stringify('outside forged'),JSON.parse(r.result).documents[1].id);}));
      await p.evaluate(()=>velocimd.action('theme',0));await save(p);assert.equal((await state(p)).error,null);
      await p.reload();await p.waitForFunction(()=>document.documentElement.dataset.ready==='true');assert((await state(p)).error.includes('corrupt'));assert((await state(p)).recovery);
      assert((await p.evaluate(()=>velocimd.recovery_drafts())).includes('outside forged'));
    }finally{await ctx.close();}
  });
  await test('near-limit actual input serializes/writes only changed body; metadata serializes/writes zero bodies',async()=>{
    const documents=[{title:'small.md',content:'small\n'},...Array.from({length:15},(_,i)=>({title:`large-${i}.md`,content:'x'.repeat(270000)}))];
    const raw=session(documents);const {p,ctx}=await newPage(({raw})=>{if(!sessionStorage.seeded){localStorage.setItem('velocimd.browser-drafts.v1',raw);sessionStorage.seeded='1';}},{raw});
    try {
      assert.equal((await state(p)).error,null);await save(p);
      await assert.rejects(()=>p.evaluate(()=>velocimd.action('new',0)),/At most 16 browser tabs/);
      const before=(await state(p)).persistence;
      await p.evaluate(()=>{
        window.storeOps=[];window.parseCount=0;window.stringifyCount=0;window.longTasks=[];
        const parse=JSON.parse,stringify=JSON.stringify;window.restoreJSON=()=>{JSON.parse=parse;JSON.stringify=stringify;};
        JSON.parse=function(...a){if(new Error().stack.includes('/browser-store.js:'))parseCount++;return parse(...a);};
        JSON.stringify=function(...a){if(new Error().stack.includes('/browser-store.js:'))stringifyCount++;return stringify(...a);};
        for(const name of ['get','getAll','getAllKeys','put','delete']){const f=IDBObjectStore.prototype[name];IDBObjectStore.prototype[name]=function(...a){storeOps.push({op:name,store:this.name,key:name==='put'?a[1]:a[0],bytes:name==='put'&&typeof a[0]==='string'?new TextEncoder().encode(a[0]).length:0});return f.apply(this,a);};}
        if(globalThis.PerformanceObserver?.supportedEntryTypes.includes('longtask'))new PerformanceObserver(list=>longTasks.push(...list.getEntries().map(e=>({start:e.startTime,duration:e.duration})))).observe({type:'longtask'});
        performance.mark('issue11-edit-start');
      });
      await edit(p,'one small edit ');await save(p);
      const after=(await state(p)).persistence;
      const trace=await p.evaluate(()=>{performance.mark('issue11-edit-ack');const duration=performance.measure('issue11-input-through-ack','issue11-edit-start','issue11-edit-ack').duration;restoreJSON();return {duration,operations:storeOps,parser_calls:parseCount,stringify_calls:stringifyCount,longTasks:longTasks.slice(0,100)};});
      assert.equal(after.serialized_bodies-before.serialized_bodies,1);
      assert.equal(trace.operations.filter(o=>o.op==='put'&&o.store==='documents').length,1);
      assert.equal(trace.operations.filter(o=>['getAll','getAllKeys'].includes(o.op)).length,0);
      assert.equal(trace.operations.filter(o=>o.op==='get'&&o.store==='documents').length,2);
      await p.evaluate(()=>{storeOps.length=0;velocimd.action('theme',0);velocimd.action('flush',0);});await settled(p);
      const metadata=(await state(p)).persistence;const operations=await p.evaluate(()=>storeOps);
      assert.equal(metadata.serialized_bodies,after.serialized_bodies);assert.equal(operations.filter(o=>o.store==='documents').length,0);
      await writeFile(resolve(evidence,'near-limit-trace.json'),JSON.stringify({logicalLegacyBytes:Buffer.byteLength(raw),documents:16,rustBefore:before,rustAfter:after,metadata,trace,metadataOperations:operations,scope:'Actual input -> acknowledged transaction including test instrumentation, editor mode/focus and bridge overhead; not an uninstrumented speedup benchmark'},null,2));
    }finally{await ctx.close();}
  });
  assert(results.length>0,'The requested browser case filter selected no tests');
  assert.deepEqual(errors,[]);
  await writeFile(resolve(evidence,'results.json'),JSON.stringify({browser:browser.version(),results,passed:results.length,failed:0,errors},null,2));
  console.log(JSON.stringify({passed:results.length,failed:0,evidence}));
}catch(e){await writeFile(resolve(evidence,'failure.json'),JSON.stringify({results,errors,error:String(e),stack:e.stack},null,2));throw e;}
finally{await cleanup();}
