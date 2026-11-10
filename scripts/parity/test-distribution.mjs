// Exercise the ordinary importer against the actual final ZIP, not invented receipts.
import assert from 'node:assert/strict'
import {readFile,writeFile,mkdtemp,rm,mkdir} from 'node:fs/promises'
import {join,resolve,dirname} from 'node:path'
import {execFileSync} from 'node:child_process'
import fsPromises from 'node:fs/promises'
import {syncBuiltinESMExports} from 'node:module'
import {importDistribution,validateDistribution,sha} from '../browser-artifact.mjs'

const record=JSON.parse(await readFile(resolve(process.argv[2]),'utf8'))
assert(record.status==='passed'&&process.env.TMPDIR,'Use actual artifact.json and owned TMPDIR')
const scratch=await mkdtemp(join(process.env.TMPDIR,'parity-distribution-'))
const results=[]
const directoryHash=async(directory,pin)=>{
  const manifest=await readFile(join(directory,'build.json'))
  const names=['index.html','velocimd.js','velocimd_bg.wasm','browser-store.js','source.zip','build.json']
  return {manifest:sha(manifest),pin:sha(await readFile(pin)),files:Object.fromEntries(await Promise.all(names.map(async name=>[name,sha(await readFile(join(directory,name)))])))}
}
try{
  const target=join(scratch,'public/demo'),pinPath=join(scratch,'demo-artifact.json')
  const pin=await importDistribution({archive:record.archive,expectedSha256:record.archiveSha256,target,pinPath})
  await validateDistribution(target,{pin,requirePin:true})
  results.push({id:'actual-final-archive-import-round-trip',status:'passed'})
  const before=await directoryHash(target,pinPath)
  for(const kind of ['asset-corruption','missing-proof','failed-proof','missing-safety-gates','missing-command-journeys']){
    const bad=join(scratch,kind+'.zip')
    const program=`import sys,zipfile,json
with zipfile.ZipFile(sys.argv[1]) as source,zipfile.ZipFile(sys.argv[2],'w',compression=zipfile.ZIP_DEFLATED) as output:
 for name in source.namelist():
  data=source.read(name)
  if sys.argv[3]=='asset-corruption' and name=='index.html':data+=b'<!-- deliberate private negative control -->'
  if name=='build.json' and sys.argv[3] in ['missing-proof','failed-proof','missing-safety-gates','missing-command-journeys']:
   m=json.loads(data)
   if sys.argv[3]=='missing-proof':del m['verification']
   elif sys.argv[3]=='failed-proof':m['verification']['gates'][0]['exitCode']=9
   elif sys.argv[3]=='missing-safety-gates':m['verification']['gates']=[g for g in m['verification']['gates'] if not g['id'].startswith('browser-regressions-')]
   else:
    for g in m['verification']['gates']:
     if g['id'] in ['native-ui','browser-chromium','browser-firefox']:
      g['cases']=[c for c in g['cases'] if c['id'] in m['verification']['matrix']['caseIds']]
      g['caseCount']=len(g['cases'])
   data=(json.dumps(m,indent=2)+'\\n').encode()
  output.writestr(name,data)
`
    execFileSync(process.env.PYTHON||'python3',['-c',program,record.archive,bad,kind],{encoding:'utf8',timeout:30000})
    const badSha=sha(await readFile(bad))
    await assert.rejects(()=>importDistribution({archive:bad,expectedSha256:badSha,target,pinPath}),kind==='asset-corruption'?/asset (size|hash) mismatch/:kind==='missing-proof'?/verification/:kind==='failed-proof'?/gate exit/:/required gate|journey\/case inventory/)
    assert.deepEqual(await directoryHash(target,pinPath),before,'Rejected input changed the accepted bundle/pin')
    results.push({id:kind+'-rejected-with-exact-predecessor-retained',status:'passed'})
  }
  // Change only the caller-owned ZIP after the real approval read completes.
  // The replacement is independently valid but has different build.json bytes.
  const raced=join(scratch,'race-input.zip'),alternate=join(scratch,'alternate.zip')
  const originalBytes=await readFile(record.archive)
  await writeFile(raced,originalBytes,{flag:'wx'})
  execFileSync(process.env.PYTHON||'python3',['-c',`import sys,zipfile
with zipfile.ZipFile(sys.argv[1]) as source,zipfile.ZipFile(sys.argv[2],'w',compression=zipfile.ZIP_DEFLATED) as output:
 for name in source.namelist():output.writestr(name,source.read(name)+(b'\\n' if name=='build.json' else b''))
`,record.archive,alternate],{encoding:'utf8',timeout:30000})
  const alternateBytes=await readFile(alternate)
  assert.notEqual(sha(alternateBytes),record.archiveSha256)
  const alternateTarget=join(scratch,'alternate-target/demo'),alternatePin=join(scratch,'alternate-target/pin.json')
  const alternative=await importDistribution({archive:alternate,expectedSha256:sha(alternateBytes),target:alternateTarget,pinPath:alternatePin})
  assert.notEqual(alternative.manifestSha256,record.manifestSha256,'Replacement control must be distinct and valid')
  const originalRead=fsPromises.readFile
  let swapped=false
  fsPromises.readFile=async(path,...args)=>{
    const bytes=await originalRead(path,...args)
    if(path===raced&&!swapped){swapped=true;await writeFile(raced,alternateBytes)}
    return bytes
  }
  syncBuiltinESMExports()
  try{
    const racePin=await importDistribution({archive:raced,expectedSha256:record.archiveSha256,target:join(scratch,'race-target/demo'),pinPath:join(scratch,'race-target/pin.json')})
    assert(swapped,'Controlled pathname rewrite must actually execute')
    assert.equal(racePin.manifestSha256,record.manifestSha256,'Importer consumed replacement bytes instead of the approved snapshot')
  }finally{fsPromises.readFile=originalRead;syncBuiltinESMExports()}
  assert.deepEqual(await directoryHash(target,pinPath),before)
  results.push({id:'archive-rewrite-after-approval-installs-only-approved-snapshot',status:'passed'})
  const stale=JSON.parse(await readFile(pinPath,'utf8'));stale.sourceSha256='0'.repeat(64)
  await assert.rejects(()=>validateDistribution(target,{pin:stale,requirePin:true}),/Source differs/)
  results.push({id:'stale-source-pin-rejected',status:'passed'})
  await mkdir(join(scratch,'.run/browser-import'),{recursive:true})
  await writeFile(join(scratch,'.run/browser-import/import.lock'),'owned competing-import fixture',{flag:'wx'})
  await assert.rejects(()=>importDistribution({archive:record.archive,expectedSha256:record.archiveSha256,target,pinPath}),/EEXIST/)
  assert.deepEqual(await directoryHash(target,pinPath),before)
  results.push({id:'competing-import-refused-with-predecessor-retained',status:'passed'})
  const report={schema:1,status:'passed',archiveSha256:record.archiveSha256,sourceSha256:record.sourceSha256,caseCount:results.length,cases:results}
  await writeFile(join(dirname(resolve(process.argv[2])),'post-package-checks.json'),JSON.stringify(report,null,2)+'\n',{flag:'wx'})
  console.log(JSON.stringify(report,null,2))
}finally{await rm(scratch,{recursive:true})}
