// Focused real-canvas responsive gate. Build/serve the candidate first, or let
// WEB_ROOT (default target/web) select an already built artifact. No build here.
// RESPONSIVE_URL + FRAME_SELECTOR='.browser-frame iframe' tests a site's actual
// embedded app in the same page, rather than opening a competing origin writer.
// --unit exercises only the geometry oracle; it is NOT browser acceptance.
import assert from 'node:assert/strict';
import {resolve} from 'node:path';
import {mkdir,writeFile} from 'node:fs/promises';

const right = r => r[0] + r[2];
const bottom = r => r[1] + r[3];
const close = (a,b) => Math.abs(a-b) <= 1;
export function assertResponsiveGeometry(s) {
  const {app, editor_pane:e, preview_pane:p, divider:d, divider_hit:hit} = s.regions;
  assert(app && e && p && d && hit, 'missing measured app/panes/divider');
  for (const r of [app,e,p,d,hit]) {
    assert(r.every(Number.isFinite) && r[2] >= 0 && r[3] >= 0, 'invalid rectangle');
    assert(r[0] >= -1 && right(r) <= s.viewport[0]+1, 'horizontal overflow');
    assert(r[1] >= -1 && bottom(r) <= s.viewport[1]+1, 'vertical overflow');
  }
  const stacked = app[2] < 720 || app[2] < app[3];
  if (stacked) {
    assert(close(e[0],p[0]) && close(e[2],p[2]), 'stacked pane widths differ');
    assert(close(bottom(e),d[1]) && close(bottom(d),p[1]), 'expected editor above preview');
    assert(hit[3] >= Math.min(24,bottom(p)-e[1]), 'short stacked hit target');
  } else {
    assert(close(e[1],p[1]) && close(e[3],p[3]), 'landscape pane heights differ');
    assert(close(right(e),d[0]) && close(right(d),p[0]), 'expected side-by-side panes');
    assert.equal(d[2],8, 'desktop divider geometry changed');
  }
  const toolbar=s.controls.filter(c=>c.id.startsWith('toolbar:'));
  assert.equal(toolbar.length,11, 'a toolbar command disappeared');
  for (const c of [...toolbar,...s.controls.filter(c=>c.id==='filename')]) {
    assert(c.rect[0] >= 0 && right(c.rect) <= s.viewport[0]+1, `offscreen ${c.id}`);
    assert(c.rect[3] > 0, `empty control ${c.id}`);
    if(c.id.startsWith('toolbar:')) {
      assert.equal(c.rect[3],28, `changed icon height ${c.id}`);
      assert.equal(c.rect[2],30, `changed icon width ${c.id}`);
    }
  }
  assert(s.controls.some(c=>c.id==='filename'), 'filename editor disappeared');
  return stacked;
}

if (process.argv.includes('--unit')) {
  const {test}=await import('node:test');
  const fixture=()=>({viewport:[390,844],regions:{app:[8,8,374,828],editor_pane:[8,160,374,300],divider:[8,460,374,24],divider_hit:[8,460,374,24],preview_pane:[8,484,374,352]},controls:[...Array.from({length:11},(_,i)=>({id:`toolbar:${i}`,rect:[16+(i%7)*38,62+Math.floor(i/7)*32,30,28]})),{id:'filename',rect:[16,126,236,28]}]});
  test('oracle accepts a measured-shape stacked fixture',()=>assert.equal(assertResponsiveGeometry(fixture()),true));
  test('oracle rejects a side-by-side regression on a phone',()=>{const s=fixture();s.regions.preview_pane=[200,160,182,676];assert.throws(()=>assertResponsiveGeometry(s));});
  test('oracle rejects a missing command',()=>{const s=fixture();s.controls.shift();assert.throws(()=>assertResponsiveGeometry(s));});
  test('oracle rejects offscreen narrow chrome',()=>{const s=fixture();s.controls[10].rect[0]=380;assert.throws(()=>assertResponsiveGeometry(s));});
} else {
  const {createRequire}=await import('node:module');
  const require=createRequire(import.meta.url);
  const {chromium,firefox}=require(process.env.PLAYWRIGHT_MODULE || 'playwright');
  const browserName=process.env.BROWSER || 'chromium';
  assert(['chromium','firefox'].includes(browserName));
  const scale=Number(process.env.DEVICE_SCALE_FACTOR||1);
  assert(Number.isFinite(scale)&&scale>=1&&scale<=4,'test scale must be in [1,4]');
  let service, browser;
  const errors=[];
  const results=[];
  const evidence=process.env.EVIDENCE_DIR;
  if(evidence)await mkdir(evidence,{recursive:true});
  try {
    let url=process.env.RESPONSIVE_URL;
    if(!url){
      const {serveWeb}=await import('./parity/serve.mjs');
      service=await serveWeb(resolve(process.env.WEB_ROOT || 'target/web'));
      url=service.url;
    }
    const engine=browserName==='firefox'?firefox:chromium;
    // Playwright's DPR override alone does not change devicePixelContentBoxSize.
    // Match the browser's native scale too, as on a real high-DPI screen.
    browser=await engine.launch({headless:process.env.HEADED!=='1',...(browserName==='chromium'?{chromiumSandbox:true,args:[`--force-device-scale-factor=${scale}`]}:{firefoxUserPrefs:{'layout.css.devPixelsPerPx':String(scale)}}),...(process.env.CHROME?{executablePath:process.env.CHROME}:{})});
    const context=await browser.newContext({viewport:{width:1280,height:800},hasTouch:true,deviceScaleFactor:scale,...(browserName==='chromium'?{isMobile:process.env.MOBILE==='1'}:{})});
    const page=await context.newPage();
    page.on('pageerror',error=>errors.push(String(error)));
    await page.goto(url);
    const target=process.env.FRAME_SELECTOR
      ? await (await page.waitForSelector(process.env.FRAME_SELECTOR)).contentFrame()
      : page;
    assert(target,'missing requested app iframe');
    await target.waitForFunction(()=>document.documentElement.dataset.ready==='true',null,{timeout:60000});
    await target.evaluate(()=>velocimd.set_ui_probe(true));
    const frame=()=>target.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
    const snapshot=()=>target.evaluate(()=>JSON.parse(velocimd.ui_snapshot()));
    const state=()=>target.evaluate(()=>JSON.parse(velocimd.snapshot()));
    async function settled(){for(let i=0;i<5;i++)await frame();return snapshot();}
    async function point(x,y){await target.locator('canvas').scrollIntoViewIfNeeded();const b=await target.locator('canvas').boundingBox();const s=await snapshot();return {x:b.x+x*b.width/s.viewport[0],y:b.y+y*b.height/s.viewport[1]};}
    async function click(id){const s=await snapshot();const c=s.controls.find(c=>c.id===id);assert(c?.enabled,`missing/disabled ${id}`);const p=await point(c.rect[0]+c.rect[2]/2,c.rect[1]+c.rect[3]/2);await page.mouse.click(p.x,p.y);await frame();}
    const content='alpha e\u0301 🦀\r\n'+Array.from({length:80},(_,i)=>`\n## Section ${i}\n\nParagraph ${i}.\n`).join('');
    await target.locator('#file').setInputFiles({name:'responsive.md',mimeType:'text/markdown',buffer:Buffer.from(content)});
    await target.waitForFunction(s=>JSON.parse(velocimd.active_document()).content===s,content);
    await settled();
    if((await state()).mode!=='Split')await click('toolbar:split');
    const initialRatio=(await settled()).split_ratio;
    assert(Number.isFinite(initialRatio), 'missing measured split ratio');
    // Selection is made with real keyboard events, retained while dimensions change.
    const editor=(await settled()).regions.editor;
    const focus=await point(editor[0]+80,editor[1]+30);
    await page.mouse.click(focus.x,focus.y);
    await page.keyboard.press('Control+Home');await page.keyboard.press('Shift+ArrowRight');
    for(const [width,height] of [[320,720],[360,780],[390,844],[430,260],[800,1000],[1280,800]]){
      await page.setViewportSize({width,height});
      await target.waitForFunction(()=>JSON.parse(velocimd.ui_snapshot()).controls?.length>0);
      const s=await settled();
      const css=await target.locator('canvas').boundingBox();
      assert(Math.abs(s.viewport[0]-css.width)<1&&Math.abs(s.viewport[1]-css.height)<1,'browser DPR/resize-observer dimensions disagree');
      if(evidence){
        await page.screenshot({path:resolve(evidence,`${width}x${height}-before.png`)});
        await writeFile(resolve(evidence,`${width}x${height}.json`),JSON.stringify(s,null,2)+'\n');
      }
      const stacked=assertResponsiveGeometry(s);
      const current=await state();
      assert.equal(current.mode,'Split');assert.equal(s.split_ratio,initialRatio);
      assert.equal(s.active_content,content,'passive resize edited source');
      results.push({host:[width,height],app:s.viewport,stacked,sourcePreserved:true});
      if(evidence){
        await target.locator('canvas').scrollIntoViewIfNeeded();
        await page.screenshot({path:resolve(evidence,`${width}x${height}.png`)});
        await writeFile(resolve(evidence,`${width}x${height}.json`),JSON.stringify(s,null,2)+'\n');
      }
      console.log(`PASS host ${width}x${height}, app ${s.viewport.join('x')}, ${stacked?'stacked':'side-by-side'}`);
    }
    await page.keyboard.insertText('猫');
    await target.waitForFunction(s=>JSON.parse(velocimd.active_document()).content===s,'猫'+content.slice(1));
    // Exercise real mouse and Chromium's native touch route, never dispatchEvent.
    for(const touch of [false,true]){
      if(touch&&browserName!=='chromium')continue;
      await page.setViewportSize({width:390,height:844});
      const s=await settled();assertResponsiveGeometry(s);
      const e=s.regions.editor_pane,p=s.regions.preview_pane,d=s.regions.divider;
      const start=await point(d[0]+d[2]/2,d[1]+d[3]/2);
      const end=await point(d[0]+d[2]/2,d[1]+d[3]/2+55);
      if(touch){
        const cdp=await context.newCDPSession(page);
        try {
          await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:start.x,y:start.y}]});await frame();
          await cdp.send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:end.x,y:end.y}]});await frame();
          await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
        }finally{await cdp.detach();}
      }else{
        await page.mouse.move(start.x,start.y);await page.mouse.down();await page.mouse.move(end.x,end.y,{steps:8});await page.mouse.up();
      }
      await settled();
      const extent=bottom(p)-e[1]-d[3];
      const min=Math.min(0.5,180/extent);
      const expected=Math.max(min,Math.min(1-min,0.85,Math.max(0.15,(d[1]+55-e[1])/extent)));
      assert(Math.abs((await snapshot()).split_ratio-expected)<0.005,`${touch?'touch':'mouse'} ratio did not follow y`);
      const reset=(await snapshot()).regions.divider;
      const at=await point(reset[0]+reset[2]/2,reset[1]+reset[3]/2);
      await page.mouse.dblclick(at.x,at.y);await settled();
      assert.equal((await snapshot()).split_ratio,0.5);
      results.push({drag:touch?'native-CDP-touch':'mouse',reset:true});
      console.log(`PASS stacked ${touch?'touch':'mouse'} drag and double-click reset`);
    }
    // The same Split action enters automatic mode, locks, then unlocks.
    await target.evaluate(()=>{
      window.splitKeyTrace=[];
      for(const type of ['keydown','keyup'])document.addEventListener(type,e=>{
        if(e.code==='Digit3')queueMicrotask(()=>window.splitKeyTrace.push({type,key:e.key,repeat:e.repeat,ctrl:e.ctrlKey,prevented:e.defaultPrevented,target:e.target.tagName}));
      },true);
    });
    for(const input of ['pointer','keyboard']){
      for(const startSize of [[1280,800],[390,844]]){
        const originalIsStacked=startSize[0]<720;
        await page.setViewportSize({width:startSize[0],height:startSize[1]});await settled();
        await click('toolbar:edit');await settled();
        if(input==='keyboard'){
          const r=(await snapshot()).regions.editor;
          const at=await point(r[0]+60,r[1]+25);await page.bringToFront();await page.mouse.click(at.x,at.y);
          await target.waitForFunction(()=>document.hasFocus()&&document.activeElement instanceof HTMLInputElement&&!document.activeElement.id);
        }
        const activate=async()=>{
          const before=await snapshot();const mode=(await state()).mode;
          const locked=mode==='Split'?!before.split_locked:false;
          if(input==='pointer')await click('toolbar:split');
          else{
            // Give the frame-driven canvas adapter the modifier/focus phase
            // before the digit, as in a physical chord; never retry the action.
            await page.keyboard.down('Control');await frame();
            await page.keyboard.press('3');await page.keyboard.up('Control');await frame();
          }
          try{
            await target.waitForFunction(locked=>{
              const s=JSON.parse(velocimd.ui_snapshot());
              return s.mode==='Split'&&s.split_locked===locked;
            },locked,{timeout:5000});
          }catch(error){
            if(evidence)await writeFile(resolve(evidence,'shortcut-failure.json'),JSON.stringify({input,startSize,before,after:await snapshot(),trace:await target.evaluate(()=>window.splitKeyTrace)},null,2)+'\n');
            throw error;
          }
          await settled();
        };
        await activate();
        assert.equal((await state()).mode,'Split');assert.equal((await snapshot()).split_locked,false);
        await activate();assert.equal((await snapshot()).split_locked,true);
        const other=originalIsStacked?{width:1280,height:800}:{width:390,height:844};
        await page.setViewportSize(other);await settled();
        const locked=await snapshot();const e=locked.regions.editor_pane,p=locked.regions.preview_pane;
        assert.equal(bottom(e)<=p[1],originalIsStacked,`${input}: locked orientation changed on resize`);
        assert.equal(locked.active_content,'猫'+content.slice(1));
        if(evidence&&input==='pointer')await page.screenshot({path:resolve(evidence,originalIsStacked?'locked-over-under-wide.png':'locked-side-by-side-phone.png')});
        // Divider resize and reset remain available while orientation is locked.
        const d=locked.regions.divider;const at=await point(d[0]+d[2]/2,d[1]+d[3]/2);
        await page.mouse.move(at.x,at.y);await page.mouse.down();
        await page.mouse.move(at.x+(originalIsStacked?0:25),at.y+(originalIsStacked?25:0),{steps:8});await page.mouse.up();await settled();
        const dragged=await snapshot();
        const extent=originalIsStacked?e[3]+p[3]:e[2]+p[2];
        const minimum=Math.min(0.5,180/extent);
        const effectiveBefore=originalIsStacked?e[3]/extent:e[2]/extent;
        const expected=Math.max(0.15,Math.min(0.85,Math.max(minimum,Math.min(1-minimum,effectiveBefore+25/extent))));
        assert(Math.abs(dragged.split_ratio-expected)<0.001,`${input}: locked divider did not follow its axis`);
        if(Math.abs(expected-locked.split_ratio)>0.00001)assert(Math.abs(dragged.split_ratio-locked.split_ratio)>0.00001,'locked drag was a no-op');
        assert.equal(dragged.split_locked,true);
        const reset=dragged.regions.divider;const resetAt=await point(reset[0]+reset[2]/2,reset[1]+reset[3]/2);
        await page.mouse.dblclick(resetAt.x,resetAt.y);await settled();
        assert.equal((await snapshot()).split_ratio,0.5);assert.equal((await snapshot()).split_locked,true);
        // Restore editor focus after testing the divider, so real shortcut input
        // goes to the app rather than the browser's tab-selection accelerator.
        if(input==='keyboard'){
          const r=(await snapshot()).regions.editor;const at=await point(r[0]+60,r[1]+25);await page.bringToFront();await page.mouse.click(at.x,at.y);
          await target.waitForFunction(()=>document.hasFocus()&&document.activeElement instanceof HTMLInputElement&&!document.activeElement.id);
        }
        await activate();assert.equal((await snapshot()).split_locked,false);assertResponsiveGeometry(await snapshot());
        assert.equal((await snapshot()).active_content,'猫'+content.slice(1));
        results.push({input,lockedOrientation:originalIsStacked?'over-under':'side-by-side',resizePreserved:true,unlockAutomatic:true});
        console.log(`PASS ${input}: enter, lock ${originalIsStacked?'over-under':'side-by-side'}, resize, unlock`);
      }
    }
    assert.deepEqual(errors,[],'browser page errors');
    if(evidence)await writeFile(resolve(evidence,'results.json'),JSON.stringify({browser:browserName,version:browser.version(),results,selectionAcrossResize:true,errors,physicalDeviceCoverage:false},null,2)+'\n');
    console.log('PASS source selection replacement across resize; no physical iOS keyboard/IME claim');
  } finally {
    await browser?.close();
    if(service)await service.close();
  }
}
