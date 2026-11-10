import assert from 'node:assert/strict'
import {execFileSync} from 'node:child_process'
import {readFile,writeFile,mkdir,copyFile} from 'node:fs/promises'
import {resolve,join,dirname} from 'node:path'
import {fileURLToPath} from 'node:url'
import {inventorySource,archiveSource,sourcePackages} from './source.mjs'
import {RUNTIME_ASSETS} from '../parity-proof.mjs'
import {sha,validateDistribution,validateBuildPolicy,DISTRIBUTION_FILES} from '../browser-artifact.mjs'

const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..')
const args=Object.fromEntries(process.argv.slice(2).map((value,index,array)=>value.startsWith('--')?[value.slice(2),array[index+1]]:[]).filter(item=>item.length))
assert(args.source&&args.report&&args.output,'Specify --source, --report and --output')
const source=JSON.parse(await readFile(resolve(args.source),'utf8')),report=JSON.parse(await readFile(resolve(args.report),'utf8'))
assert.equal(report.status,'passed','Cannot package an incomplete or failed pipeline')
assert.equal(report.sourceSha256,source.sha256,'Source/receipt mismatch')
assert.deepEqual(await inventorySource(root),source,'Source changed while verification ran')
const output=resolve(args.output),folder=join(output,'web')
await mkdir(folder,{recursive:true})
const files={}
for(const name of RUNTIME_ASSETS){
  const input=join(root,'target/web',name),bytes=await readFile(input)
  assert.equal(sha(bytes),report.assets[name],'Packaged asset was not the browser-tested asset: '+name)
  await copyFile(input,join(folder,name))
  files[name]={bytes:bytes.length,sha256:sha(bytes)}
}
const buildPolicy=JSON.parse(await readFile(join(root,'target/web/build-policy.json'),'utf8'))
validateBuildPolicy(buildPolicy,files['velocimd_bg.wasm'])
await archiveSource(root,source,join(folder,'source.zip'))
const sourceBytes=await readFile(join(folder,'source.zip'))
files['source.zip']={bytes:sourceBytes.length,sha256:sha(sourceBytes)}
const version=execFileSync(process.env.PYTHON||'python3',['-c','import tomllib,sys;print(tomllib.load(open(sys.argv[1],"rb"))["package"]["version"])',join(root,'Cargo.toml')],{encoding:'utf8'}).trim()
const gates=report.gates.map(({id,status,exitCode,caseCount,cases,durationMs,stdoutSha256,stderrSha256,version})=>({id,status,exitCode,...(cases?{caseCount,cases}:{}),...(durationMs!==undefined?{durationMs}:{}),...(stdoutSha256?{stdoutSha256,stderrSha256}:{}),...(version?{version}:{})}))
const verification={schema:1,status:'passed',sourceSha256:source.sha256,assets:report.assets,matrix:report.matrix,consoleErrors:report.consoleErrors,gates,tools:report.tools}
const manifest={format:1,version,license:'AGPL-3.0-only',source,rustPackages:await sourcePackages(root),files,verification,buildPolicy}
await writeFile(join(folder,'build.json'),JSON.stringify(manifest,null,2)+'\n',{flag:'wx'})
await validateDistribution(folder)
const name=`velocimd-web-${version}.zip`,archive=join(output,name)
const zip=`import sys,pathlib,json,zipfile
root=pathlib.Path(sys.argv[1]);names=json.loads(sys.argv[3])
with zipfile.ZipFile(sys.argv[2],'x') as z:
 for name in names:
  info=zipfile.ZipInfo(name);info.create_system=3;info.external_attr=0o100644<<16
  z.writestr(info,(root/name).read_bytes(),compress_type=zipfile.ZIP_DEFLATED,compresslevel=9)
`
execFileSync(process.env.PYTHON||'python3',['-c',zip,folder,archive,JSON.stringify(DISTRIBUTION_FILES)],{encoding:'utf8',timeout:30000})
const archiveSha256=sha(await readFile(archive))
await writeFile(join(output,'SHA256SUMS'),`${archiveSha256}  ${name}\n`,{flag:'wx'})
const result={schema:1,status:'passed',archive,archiveSha256,manifestSha256:sha(await readFile(join(folder,'build.json'))),sourceSha256:source.sha256,baseCommit:source.baseCommit,modifiedBrowserBuild:source.modifiedBrowserBuild}
await writeFile(join(output,'artifact.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'})
console.log(JSON.stringify(result,null,2))
