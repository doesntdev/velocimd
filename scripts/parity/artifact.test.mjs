import test from 'node:test'
import assert from 'node:assert/strict'
import {mkdtemp,rm,writeFile,readFile,mkdir} from 'node:fs/promises'
import {join} from 'node:path'
import {safeSourcePath,importDistribution,sha} from '../browser-artifact.mjs'
import {execFileSync} from 'node:child_process'

test('source names reject traversal, control characters, secrets and generated roots without normalizing tokens',()=>{
  for(const name of ['../Cargo.toml','/src/lib.rs','src\\lib.rs','src/../secret','src/./lib.rs','src//lib.rs','.env','src/.env.local','src/\u0000file','C:/private','target/app.wasm','graphify-out/graph.json'])assert.equal(safeSourcePath(name),false,name)
  for(const name of ['src/lib.rs','Cargo.lock','.github/workflows/release.yml','assets/real name.svg'])assert.equal(safeSourcePath(name),true,name)
})
async function workspace(fn){
  assert(process.env.TMPDIR,'Use owned TMPDIR for importer fixtures')
  const root=await mkdtemp(join(process.env.TMPDIR,'parity-artifact-'))
  try{
    const target=join(root,'public/demo'),pinPath=join(root,'demo-artifact.json')
    await mkdir(target,{recursive:true});await writeFile(join(target,'keep'),'previous accepted bytes')
    await writeFile(pinPath,'previous-pin')
    await fn({root,target,pinPath})
    assert.equal(await readFile(join(target,'keep'),'utf8'),'previous accepted bytes')
    assert.equal(await readFile(pinPath,'utf8'),'previous-pin')
  }finally{await rm(root,{recursive:true})}
}
test('wrong archive SHA is rejected before replacing the accepted bundle or pin',()=>workspace(async({root,target,pinPath})=>{
  const archive=join(root,'wrong.zip');await writeFile(archive,'not the approved bytes')
  await assert.rejects(()=>importDistribution({archive,expectedSha256:'0'.repeat(64),target,pinPath}),/approved SHA/)
}))
test('traversal, duplicate and incomplete ZIPs are rejected with predecessor retained',()=>workspace(async({root,target,pinPath})=>{
  for(const kind of ['traversal','duplicate','incomplete']){
    const archive=join(root,kind+'.zip')
    execFileSync('python3',['-c','import zipfile,sys;z=zipfile.ZipFile(sys.argv[1],"w");z.writestr("../escape" if sys.argv[2]=="traversal" else "index.html","unit-only invalid artifact");z.writestr("index.html","duplicate") if sys.argv[2]=="duplicate" else None;z.close()',archive,kind])
    const expectedSha256=sha(await readFile(archive))
    await assert.rejects(()=>importDistribution({archive,expectedSha256,target,pinPath}),/archive members|Command failed/)
  }
  await assert.rejects(()=>readFile(join(root,'escape')),/ENOENT/)
}))
test('malformed/nonprimitive pins are rejected rather than coerced',()=>workspace(async({root,target,pinPath})=>{
  for(const expectedSha256 of [undefined,{},new String('0'.repeat(64)),'F'.repeat(64),'0'.repeat(63)]){
    await assert.rejects(()=>importDistribution({archive:join(root,'unused'),expectedSha256,target,pinPath}))
  }
}))
