import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { validateSnapshot, compareGeometry, validateVerification, REQUIRED_GATES } from './contract.mjs'

const contract = JSON.parse(readFileSync(new URL('../../tests/parity/contract.json', import.meta.url), 'utf8'))
const sourceSha256 = '1'.repeat(64)
const assets = Object.fromEntries(['index.html', 'velocimd.js', 'velocimd_bg.wasm'].map(name => [name, '2'.repeat(64)]))
// Unit-only structured fixtures; these are never emitted as real execution receipts.
function snapshot() {
  return {schema:1,viewport:[1280,800],pixels_per_point:1,mode:'Split',light:false,split_ratio:0.5,
    active_title:'parity.md',active_content:'fixture',
    controls:contract.toolbarIds.map((id,index)=>({id,rect:[10+index*30,52,28,28],enabled:!id.endsWith('open-folder'),selected:id==='toolbar:split'})),
    regions:{top_bar:[10,10,1260,42],command_bar:[10,52,1260,38],editor:[18,98,610,692],preview:[654,98,610,692],divider:[638,90,4,700]}}
}
function proof() {
  return {schema:1,status:'passed',sourceSha256,assets:{...assets},
    matrix:{browsers:['chromium','firefox'],caseIds:contract.cases.map(item=>item.id)},
    gates:[...new Set([...REQUIRED_GATES,'browser-regressions-chromium','browser-regressions-firefox'])].map(id=>{
      const names=id==='native-ui'?[...contract.cases.map(item=>item.id),...contract.requiredJourneys.native]:['browser-chromium','browser-firefox'].includes(id)?[...contract.cases.map(item=>item.id),...contract.requiredJourneys.browser]:id.startsWith('browser-regressions-')?contract.requiredJourneys.browserRegressions:id==='visual-parity'?['native-baseline','chromium','firefox'].flatMap(target=>contract.cases.map(item=>`${target}:${item.id}`)):id==='negative-controls'?['unit-only-negative-control']:undefined
      return {id,status:'passed',exitCode:0,...(names?{cases:names.map(id=>({id,status:'passed'})),caseCount:names.length}:{})}
    }),consoleErrors:[]}
}

test('snapshot requires actual finite, positive control and pane measurements',()=>{
  validateSnapshot(snapshot(),contract)
  const invalid=snapshot();invalid.controls[0].rect[2]=0
  assert.throws(()=>validateSnapshot(invalid,contract),/rect/)
  invalid.controls[0].rect[2]=NaN
  assert.throws(()=>validateSnapshot(invalid,contract),/rect/)
})
test('missing, duplicate and visually reordered toolbar controls are rejected',()=>{
  for(const mutation of [s=>s.controls.pop(),s=>s.controls.push(s.controls[0]),s=>{s.controls[0].rect[0]=1100}]){
    const invalid=snapshot();mutation(invalid)
    assert.throws(()=>validateSnapshot(invalid,contract),/toolbar|duplicate/i)
  }
})
test('a one-point toolbar shift and pane-width drift fail geometry comparison',()=>{
  for(const mutation of [s=>s.controls[2].rect[0]+=1,s=>s.regions.editor[2]+=2]){
    const a=snapshot(),b=snapshot();mutation(b)
    assert.throws(()=>compareGeometry(a,b,contract),/geometry|rect/i)
  }
})
test('document bytes, mode, theme, scale and selection are not visual exceptions',()=>{
  for(const mutation of [s=>s.active_content+='x',s=>s.mode='Edit',s=>s.light=true,s=>s.pixels_per_point=2,s=>s.controls[6].selected=true]){
    const a=snapshot(),b=snapshot();mutation(b)
    assert.throws(()=>compareGeometry(a,b,contract))
  }
})
test('only declared platform availability differences are accepted',()=>{
  const a=snapshot(),b=snapshot();a.controls[1].enabled=true;a.controls[5].enabled=true;b.controls[5].enabled=false
  compareGeometry(a,b,contract)
  b.controls[0].enabled=false
  assert.throws(()=>compareGeometry(a,b,contract),/availability/i)
})
test('complete unit-only proof validates its exact source, assets and declared matrix',()=>{
  validateVerification(proof(),{sourceSha256,assets,contract})
})
test('absent proof, a missing gate, a duplicate gate and a failed gate are rejected',()=>{
  assert.throws(()=>validateVerification(null,{sourceSha256,assets,contract}),/verification/)
  for(const mutation of [p=>p.gates.pop(),p=>p.gates.push(p.gates[0]),p=>p.gates[0].status='failed']){
    const invalid=proof();mutation(invalid)
    assert.throws(()=>validateVerification(invalid,{sourceSha256,assets,contract}))
  }
})
test('a passed label without executed GUI fixtures or required visual comparisons is incomplete',()=>{
  for(const id of ['native-ui','browser-chromium','browser-firefox','visual-parity','negative-controls']){
    const invalid=proof(),gate=invalid.gates.find(gate=>gate.id===id);delete gate.cases;delete gate.caseCount
    assert.throws(()=>validateVerification(invalid,{sourceSha256,assets,contract}),/cases/)
  }
})
test('both browser safety-regression gates are mandatory',()=>{
  for(const id of ['browser-regressions-chromium','browser-regressions-firefox']){
    const invalid=proof();invalid.gates=invalid.gates.filter(gate=>gate.id!==id)
    assert.throws(()=>validateVerification(invalid,{sourceSha256,assets,contract}),/required gate/)
  }
})
test('fixture-only receipts and omitted real-input or safety journeys are incomplete',()=>{
  for(const id of ['native-ui','browser-chromium','browser-firefox','browser-regressions-chromium','browser-regressions-firefox']){
    const invalid=proof(),gate=invalid.gates.find(gate=>gate.id===id)
    gate.cases.pop();gate.caseCount=gate.cases.length
    assert.throws(()=>validateVerification(invalid,{sourceSha256,assets,contract}),/journey|case inventory/)
  }
})
test('a claimed passed gate with nonzero exit is not accepted',()=>{
  const invalid=proof();invalid.gates[0].exitCode=1
  assert.throws(()=>validateVerification(invalid,{sourceSha256,assets,contract}),/exit/)
})
test('stale source or served-asset identity fails closed',()=>{
  const invalid=proof();invalid.sourceSha256='3'.repeat(64)
  assert.throws(()=>validateVerification(invalid,{sourceSha256,assets,contract}),/source/)
  const stale=proof();stale.assets['index.html']='4'.repeat(64)
  assert.throws(()=>validateVerification(stale,{sourceSha256,assets,contract}),/asset/)
})
test('missing browser, missing fixture, wrong case totals and console errors are failures',()=>{
  for(const mutation of [p=>p.matrix.browsers.pop(),p=>p.matrix.caseIds.pop(),p=>{p.gates[0].caseCount=9;p.gates[0].cases=[{id:'only',status:'passed'}]},p=>p.consoleErrors.push('startup panic')]){
    const invalid=proof();mutation(invalid)
    assert.throws(()=>validateVerification(invalid,{sourceSha256,assets,contract}))
  }
})
