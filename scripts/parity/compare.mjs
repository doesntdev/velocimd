import assert from 'node:assert/strict'
import { readFile,writeFile,mkdir } from 'node:fs/promises'
import { join,resolve,dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { compareGeometry,validateSnapshot } from './contract.mjs'
import { compareRaster,eraseRegion,VISUAL_POLICY } from './visual.mjs'
import { sha } from '../browser-artifact.mjs'

const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..')
const contract=JSON.parse(await readFile(join(root,'tests/parity/contract.json'),'utf8'))
const args=Object.fromEntries(process.argv.slice(2).map((value,index,array)=>value.startsWith('--')?[value.slice(2),array[index+1]]:[]).filter(item=>item.length))
assert(args.native&&args.matrix&&args.output,'Specify --native, --matrix and --output')
const native=JSON.parse(await readFile(resolve(args.native),'utf8')),matrix=JSON.parse(await readFile(resolve(args.matrix),'utf8'))
const output=resolve(args.output);await mkdir(output,{recursive:true})
const report={schema:1,status:'running',cases:[],negativeControls:[],policy:VISUAL_POLICY}
try {
  assert.equal(native.status,'passed');assert.equal(native.exitCode,0);assert.equal(matrix.status,'passed')
  assert.deepEqual(native.cases.map(item=>item.id).sort(),contract.cases.map(item=>item.id).sort())
  assert.deepEqual(matrix.reports.map(item=>item.engine).sort(),['chromium','firefox'])
  const baseline=JSON.parse(await readFile(join(root,'tests/parity/baseline/baseline.json'),'utf8'))
  assert.equal(baseline.nativeBinarySha256,contract.reference.nativeBinarySha256)
  assert.equal(baseline.fixtureSha256,sha(await readFile(join(root,'tests/parity/fixture.md'))))
  for(const item of native.cases){
    validateSnapshot(item.snapshot,contract)
    const reference=join(root,'tests/parity/baseline',item.id+'.png')
    assert.equal(sha(await readFile(reference)),baseline.files[item.id+'.png'],'Native baseline changed without its explicit identity')
    const metrics=await compareRaster(reference,item.image,{diffPath:join(output,'native-baseline-'+item.id+'.png'),regions:item.snapshot.regions,controls:item.snapshot.controls.filter(control=>contract.toolbarIds.includes(control.id))})
    report.cases.push({id:'native-baseline:'+item.id,status:'passed',metrics})
    for(const engine of matrix.reports){
      assert.equal(engine.status,'passed');assert.deepEqual(engine.consoleErrors,[])
      const browser=engine.cases.find(value=>value.id===item.id)
      assert(browser,'Missing browser fixture '+item.id)
      const geometry=compareGeometry(item.snapshot,browser.snapshot,contract)
      const masks=[]
      // Only the explicitly different navigation data and capability-disabled controls.
      // Shared header/bar/pane dimensions and every common command remain measured.
      for(const control of item.snapshot.controls){
        if(/^tab:\d+$/.test(control.id))masks.push({id:control.id,reason:'native folder vs browser document navigation data',rect:control.rect})
        const other=browser.snapshot.controls.find(value=>value.id===control.id)
        if(['toolbar:open-folder','toolbar:close'].includes(control.id)&&other&&other.enabled!==control.enabled)masks.push({id:control.id,reason:'declared platform availability',rect:control.rect})
      }
      const controls=item.snapshot.controls.filter(control=>contract.toolbarIds.includes(control.id)&&!masks.some(mask=>mask.id===control.id))
      const metrics=await compareRaster(item.image,browser.image,{diffPath:join(output,engine.engine+'-'+item.id+'.png'),masks:masks.map(mask=>mask.rect),regions:item.snapshot.regions,controls})
      report.cases.push({id:engine.engine+':'+item.id,status:'passed',geometry,masks,metrics})
    }
  }
  // Calibrate the ordinary oracles with private mutations. Product/source/baselines are untouched.
  const sample=native.cases[0]
  const shifted=structuredClone(sample.snapshot)
  shifted.controls.find(control=>control.id==='toolbar:new').rect[0]+=1
  assert.throws(()=>compareGeometry(sample.snapshot,shifted,contract),/Geometry/)
  report.negativeControls.push({id:'one-point-control-drift-rejected',status:'passed'})
  const missing=join(output,'negative-missing-toolbar.png')
  await eraseRegion(sample.image,missing,sample.snapshot.regions.command_bar)
  await assert.rejects(()=>compareRaster(sample.image,missing,{regions:sample.snapshot.regions,controls:sample.snapshot.controls.filter(control=>contract.toolbarIds.includes(control.id))}),/Visual parity/)
  report.negativeControls.push({id:'erased-toolbar-rejected-by-ordinary-raster-verifier',status:'passed'})
  const good=await compareRaster(sample.image,sample.image,{regions:sample.snapshot.regions})
  assert.equal(good.mismatchPixels,0)
  report.negativeControls.push({id:'unchanged-positive-raster-control',status:'passed'})
  report.status='passed'
} catch(error) {report.status='failed';report.error=String(error);if(error.metrics)report.failedMetrics=error.metrics;throw error}
finally {report.caseCount=report.cases.length;await writeFile(join(output,'report.json'),JSON.stringify(report,null,2)+'\n')}
