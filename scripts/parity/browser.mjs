// Real canvas input and browser adapters; fixture seeding is initialization, never a proxy control.
import assert from 'node:assert/strict'
import { chromium, firefox } from 'playwright'
import { readFile, writeFile, mkdir, lstat } from 'node:fs/promises'
import { resolve, join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { serveWeb } from './serve.mjs'
import { validateSnapshot } from './contract.mjs'
import { sha } from '../browser-artifact.mjs'

const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..')
const contract=JSON.parse(await readFile(join(root,'tests/parity/contract.json'),'utf8'))
const fixture=await readFile(join(root,'tests/parity/fixture.md'),'utf8')
const args=Object.fromEntries(process.argv.slice(2).map((value,index,array)=>value.startsWith('--')?[value.slice(2),array[index+1]]:[]).filter(item=>item.length))
assert(args.output&&args.web&&args.source,'Specify --output, --web and --source')
assert(process.env.VELOCIMD_PARITY_OWNED_DISPLAY==='1'&&process.env.DISPLAY,'Run matrix through the owned-Xvfb orchestrator')
const output=resolve(args.output),web=resolve(args.web)
await mkdir(output,{recursive:true})
const source=JSON.parse(await readFile(resolve(args.source),'utf8'))
const reports=[]
const runtimeNames=['index.html','velocimd.js','velocimd_bg.wasm','browser-store.js']
const service=await serveWeb(web)
const frame=page=>page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))))
export const readSnapshot=page=>page.evaluate(()=>{const value=window.velocimd.ui_snapshot();return value?JSON.parse(value)||{}:{}})
const state=page=>page.evaluate(()=>JSON.parse(window.velocimd.snapshot()))
async function awaitState(page,predicate,label){
  const until=Date.now()+15000
  while(Date.now()<until){const value=await readSnapshot(page);if(predicate(value))return value;await frame(page)}
  throw new Error('Timed out: '+label)
}
async function clickControl(page,id){
  const data=await readSnapshot(page),items=data.controls.filter(item=>item.id===id)
  assert.equal(items.length,1,'Missing/ambiguous actual control: '+id)
  assert(items[0].enabled,'Disabled actual control: '+id)
  const [x,y,w,h]=items[0].rect
  await page.locator('canvas').click({position:{x:x+w/2,y:y+h/2}})
  await frame(page)
}
async function clickEditor(page){
  const data=await readSnapshot(page),rect=data.regions.editor
  assert(rect,'Editor pane is absent')
  const [x,y,w,h]=rect
  await page.locator('canvas').click({position:{x:x+Math.min(100,w/2),y:y+Math.min(40,h/2)}})
  await frame(page)
}
async function openFixture(browser,expected){
  const context=await browser.newContext({viewport:{width:contract.viewport.width,height:contract.viewport.height},deviceScaleFactor:1,acceptDownloads:true})
  await context.addInitScript(({fixture,expected})=>{
    if(!localStorage.getItem('velocimd.parity.seeded')){
      localStorage.setItem('velocimd.browser-drafts.v1',JSON.stringify({version:1,documents:[{title:'parity.md',content:fixture}],active:0,mode:expected.mode,split_ratio:expected.splitRatio,light:expected.light}))
      localStorage.setItem('velocimd.parity.seeded','true')
    }
  },{fixture,expected})
  context.setDefaultTimeout(20000)
  const page=await context.newPage()
  page.setDefaultNavigationTimeout(60000)
  return {page,context}
}
async function ready(page){
  await page.waitForFunction(()=>document.documentElement.dataset.ready==='true',null,{timeout:60000})
  await page.evaluate(()=>window.velocimd.set_ui_probe(true))
  await awaitState(page,data=>data.controls?.some(item=>item.id==='toolbar:new'),'real rendered toolbar')
  // A legal/source footer is outside the application content viewport, not a second editor toolbar.
  const size=await page.locator('canvas').boundingBox()
  assert(size&&size.width===contract.viewport.width,'Canvas width differs')
  const overhead=contract.viewport.height-size.height
  assert(overhead>=0&&overhead<=40,'Unexpected HTML chrome reduces the editor viewport')
  if(overhead){await page.setViewportSize({width:contract.viewport.width,height:contract.viewport.height+overhead});await frame(page)}
  await awaitState(page,data=>data.viewport?.[0]===contract.viewport.width&&data.viewport?.[1]===contract.viewport.height,'matched application viewport')
}
async function settledCapture(page,path){
  await page.locator('canvas').click({position:{x:1100,y:18}})
  await page.mouse.move(contract.viewport.width+40,contract.viewport.height+40)
  await frame(page)
  let previous
  for(let pass=0;pass<50;pass++){
    const bytes=await page.locator('canvas').screenshot()
    const hash=sha(bytes)
    if(hash===previous&&bytes.length>6000){await writeFile(path,bytes);return hash}
    previous=hash
    await frame(page)
  }
  throw new Error('Canvas image did not settle')
}

try {
  for(const [engineName,engine] of [['chromium',chromium],['firefox',firefox]]){
    const directory=join(output,engineName)
    await mkdir(directory,{recursive:true})
    const report={schema:1,status:'running',engine:engineName,sourceSha256:source.sha256,cases:[],checks:[],consoleErrors:[],externalRequests:[],assets:{}}
    let browser,activeContext
    try{
      const env={...process.env,LIBGL_ALWAYS_SOFTWARE:'1',GALLIUM_DRIVER:'llvmpipe'}
      if(engineName==='chromium'&&!env.CHROME_DEVEL_SANDBOX){
        const helper='/opt/google/chrome/chrome-sandbox'
        try{const info=await lstat(helper);if(info.uid===0&&(info.mode&0o4000))env.CHROME_DEVEL_SANDBOX=helper}catch(error){if(error.code!=='ENOENT')throw error}
      }
      // Match the real browser scale to the context's one-pixel contract.
      // A context-only DPR override leaves Chromium's device-pixel resize box
      // at the host scale, creating a different backing/app viewport.
      browser=await engine.launch({headless:false,env,...(engineName==='chromium'?{chromiumSandbox:true,args:['--force-device-scale-factor=1']}:{firefoxUserPrefs:{'layout.css.devPixelsPerPx':'1'}})})
      report.version=browser.version();report.sandbox=true
      const observe=page=>{
        page.on('pageerror',error=>report.consoleErrors.push(String(error)))
        page.on('console',message=>{if(message.type()==='error')report.consoleErrors.push(message.text())})
        page.on('request',request=>{const url=new URL(request.url());if(['http:','https:'].includes(url.protocol)&&url.origin!==new URL(service.url).origin)report.externalRequests.push(url.href)})
      }
      for(const expected of contract.cases){
        const {page,context}=await openFixture(browser,expected)
        activeContext=context;observe(page)
        await page.goto(service.url)
        await ready(page)
        assert.equal(await page.locator('#new,#download,#close,#theme,#documents,[data-mode]').count(),0,'Duplicate HTML editor controls remain')
        assert.equal(await page.getByRole('button').count(),0,'Permanent HTML editor button remains')
        const data=validateSnapshot(await readSnapshot(page),contract)
        assert.equal(data.active_title,'parity.md');assert.equal(data.active_content,fixture)
        assert.equal(data.mode,expected.mode);assert.equal(data.light,expected.light)
        const image=join(directory,expected.id+'.png')
        const imageSha256=await settledCapture(page,image)
        report.cases.push({id:expected.id,status:'passed',snapshot:await readSnapshot(page),image,imageSha256})
        if(expected===contract.cases.at(-1)){
          for(const name of runtimeNames){const response=await context.request.get(new URL(name,service.url).href);assert.equal(response.status(),200);report.assets[name]=sha(await response.body());assert.equal(report.assets[name],sha(await readFile(join(web,name))))}
          const check=async(id,fn)=>{await fn();report.checks.push({id,status:'passed'});console.log('PASS',engineName,id)}
          await check('actual-mode-controls-and-theme-without-source-mutation',async()=>{
            for(const [id,mode] of [['edit','Edit'],['split','Split'],['preview','Preview'],['edit','Edit']]){await clickControl(page,'toolbar:'+id);await awaitState(page,s=>s.mode===mode,'mode '+mode)}
            const light=(await readSnapshot(page)).light
            await clickControl(page,'toolbar:theme');await awaitState(page,s=>s.light!==light,'theme toggle')
            assert.equal((await readSnapshot(page)).active_content,fixture)
          })
          await check('new-rename-and-real-multiline-editor-input',async()=>{
            await clickControl(page,'toolbar:new')
            await awaitState(page,s=>s.active_title!=='parity.md','new document')
            await clickControl(page,'filename');await page.keyboard.press('Control+a');await page.keyboard.type('parity-created');await page.keyboard.press('Enter')
            await awaitState(page,s=>s.active_title==='parity-created.md','filename commit')
            await clickEditor(page);await page.keyboard.press('Control+a');await page.keyboard.insertText('# Browser parity');await page.keyboard.press('Enter');await page.keyboard.press('Enter');await page.keyboard.insertText('Actual shared editor input.')
            await awaitState(page,s=>s.active_content==='# Browser parity\n\nActual shared editor input.','exact editor bytes')
          })
          await check('split-divider-pointer-input-persists-layout',async()=>{
            await clickControl(page,'toolbar:split');const before=await awaitState(page,s=>s.regions.divider,'split divider')
            const [x,y,w,h]=before.regions.divider,box=await page.locator('canvas').boundingBox()
            await page.mouse.move(box.x+x+w/2,box.y+y+h/2);await page.mouse.down();await page.mouse.move(box.x+x+w/2+130,box.y+y+h/2,{steps:8});await page.mouse.up()
            await awaitState(page,s=>s.split_ratio>before.split_ratio+0.05,'divider changes ratio')
          })
          const imported=Buffer.from('\ufeff# Imported parity\r\nExact café 🦀\r\nNo final newline')
          await check('actual-open-file-picker-import-and-save-download-preserve-bytes',async()=>{
            const pending=page.waitForEvent('filechooser');await clickControl(page,'toolbar:open-file');const chooser=await pending
            await chooser.setFiles({name:'exact.MARKDOWN',mimeType:'text/markdown',buffer:imported})
            await awaitState(page,s=>s.active_title==='exact.MARKDOWN','file import')
            assert.deepEqual(Buffer.from((await readSnapshot(page)).active_content),imported)
            const next=page.waitForEvent('download');await clickControl(page,'toolbar:save');const download=await next
            assert.equal(download.suggestedFilename(),'exact.MARKDOWN');assert.deepEqual(await readFile(await download.path()),imported)
          })
          await check('save-as-named-copy',async()=>{
            await clickControl(page,'toolbar:save-as')
            const dialog=page.getByRole('dialog',{name:'Download Markdown copy'})
            await dialog.waitFor({state:'visible'})
            await dialog.getByLabel('Filename').fill('parity-copy.md')
            const pending=page.waitForEvent('download')
            await dialog.getByRole('button',{name:'Download copy',exact:true}).click()
            const download=await pending
            assert.equal(download.suggestedFilename(),'parity-copy.md');assert.deepEqual(await readFile(await download.path()),imported)
            assert.equal((await readSnapshot(page)).active_title,'exact.MARKDOWN')
          })
          await check('reload-restores-real-saved-draft-and-split-ratio',async()=>{
            await page.evaluate(()=>window.velocimd.action('flush',0))
            const before=await readSnapshot(page),tabsBefore=await state(page)
            await page.reload();await ready(page)
            const after=await readSnapshot(page),tabsAfter=await state(page)
            assert.equal(after.active_content,before.active_content);assert.equal(after.active_title,before.active_title)
            assert.equal(after.mode,before.mode);assert.equal(after.light,before.light);assert.equal(after.split_ratio,before.split_ratio)
            assert.deepEqual(tabsAfter.documents,tabsBefore.documents)
          })
          await check('close-requires-confirmation-cancel-keeps-draft',async()=>{
            const before=await state(page)
            const cancelled=page.waitForEvent('dialog').then(async dialog=>{assert.equal(dialog.type(),'confirm');await dialog.dismiss()})
            await Promise.all([clickControl(page,'toolbar:close'),cancelled]);await frame(page)
            assert.deepEqual((await state(page)).documents,before.documents)
            const accepted=page.waitForEvent('dialog').then(async dialog=>{assert.equal(dialog.type(),'confirm');await dialog.accept()})
            await Promise.all([clickControl(page,'toolbar:close'),accepted])
            await page.waitForFunction(count=>JSON.parse(window.velocimd.snapshot()).documents.length===count,before.documents.length-1)
          })
          await check('palette-page-keyboard-search-executes-command-without-editing-document',async()=>{
            if((await readSnapshot(page)).light)await clickControl(page,'toolbar:theme')
            const content=(await readSnapshot(page)).active_content
            await page.locator('canvas').click({position:{x:1100,y:18}})
            await page.keyboard.press('Control+k')
            await page.waitForFunction(()=>JSON.parse(window.velocimd.snapshot()).palette_open)
            await page.waitForFunction(()=>['TEXTAREA','INPUT'].includes(document.activeElement?.tagName))
            await frame(page)
            await page.keyboard.type('Switch to light theme',{delay:12});await frame(page)
            await page.screenshot({path:join(directory,'palette.png')})
            await page.keyboard.press('Enter')
            await awaitState(page,s=>s.light===true,'palette executes the selected light-theme command')
            assert.equal((await readSnapshot(page)).active_content,content)
            assert.equal((await state(page)).error,null)
          })
        }
        await context.close();activeContext=undefined
      }
      assert.deepEqual(report.consoleErrors,[]);assert.deepEqual(report.externalRequests,[])
      report.status='passed'
    }catch(error){report.status='failed';report.error=String(error);report.stack=error.stack;throw error}
    finally{
      if(activeContext)await activeContext.close().catch(()=>{})
      if(browser)await browser.close()
      report.caseCount=report.cases.length+report.checks.length
      await writeFile(join(directory,'report.json'),JSON.stringify(report,null,2)+'\n')
      reports.push(report)
    }
  }
}finally{await service.close();await writeFile(join(output,'matrix.json'),JSON.stringify({schema:1,status:reports.length===2&&reports.every(item=>item.status==='passed')?'passed':'failed',reports},null,2)+'\n')}
