import { execFileSync } from 'node:child_process'
import { readFile, lstat, mkdir } from 'node:fs/promises'
import { join, dirname } from 'node:path'
import assert from 'node:assert/strict'
import { sha, safeSourcePath } from '../browser-artifact.mjs'

const rootNames = new Set(['Cargo.toml','Cargo.lock','build.rs','Packager.toml','LICENSE','README.md','PERFORMANCE.md','RELEASE_NOTES.md','index.html','rust-toolchain.toml','package.json','pnpm-lock.yaml','.gitignore','.gitattributes'])
const sourcePrefixes = ['vendor/epaint-0.34.1/','src/','web/','assets/','scripts/','tests/','examples/','.github/workflows/','docs/']
export const isSource = name => safeSourcePath(name) && (rootNames.has(name) || sourcePrefixes.some(prefix=>name.startsWith(prefix)))

export async function inventorySource(root) {
  const git = (...args)=>execFileSync('git',args,{cwd:root,encoding:'utf8'})
  const names=[...new Set(git('ls-files','--cached','--others','--exclude-standard','-z').split('\0').filter(name=>name && isSource(name)))].sort()
  const files={}
  const missing=[]
  for(const name of names) {
    let info
    try {info=await lstat(join(root,name))} catch(error){if(error.code==='ENOENT'){missing.push(name);continue}throw error}
    assert(info.isFile() && !info.isSymbolicLink(),`Nonregular source input: ${name}`)
    files[name]=sha(await readFile(join(root,name)))
  }
  for(const name of ['Cargo.toml','Cargo.lock','src/lib.rs','src/ui.rs','web/browser-store.js','index.html','LICENSE','vendor/epaint-0.34.1/UPSTREAM.json','vendor/epaint-0.34.1/src/text/fonts.rs','vendor/epaint-0.34.1/LICENSE-MIT','vendor/epaint-0.34.1/LICENSE-APACHE','tests/parity/contract.json'])assert(files[name],`Missing build/source input: ${name}`)
  const upstream=JSON.parse(await readFile(join(root,'vendor/epaint-0.34.1/UPSTREAM.json'),'utf8'))
  for(const name of [...Object.keys(upstream.files),...Object.keys(upstream.supplemental_upstream_licenses)])assert(files['vendor/epaint-0.34.1/'+name],`Missing modified dependency source: ${name}`)
  const untracked=git('ls-files','--others','--exclude-standard','-z').split('\0').some(name=>Object.hasOwn(files,name))
  const changed=git('diff','--name-only','-z','HEAD').split('\0').some(isSource)
  return {baseCommit:git('rev-parse','HEAD').trim(),modifiedBrowserBuild:untracked||changed||missing.length>0,sha256:sha(JSON.stringify(files)),files}
}

export async function sourcePackages(root) {
  return JSON.parse(execFileSync(process.env.PYTHON||'python3',['-c',
    'import tomllib,json,sys;d=tomllib.load(open(sys.argv[1],"rb"));print(json.dumps({p["name"]:p["version"] for p in d["package"] if p["name"] in ["pulldown-cmark","wasm-bindgen","web-time","egui","eframe"]}))',join(root,'Cargo.lock')],{encoding:'utf8'}))
}

export async function archiveSource(root,source,output) {
  await mkdir(dirname(output),{recursive:true})
  const script=`import sys,json,pathlib,zipfile,hashlib
root=pathlib.Path(sys.argv[1]);m=json.load(sys.stdin)
with zipfile.ZipFile(sys.argv[2],'x') as z:
 for name,digest in m['files'].items():
  p=root/name;data=p.read_bytes();assert hashlib.sha256(data).hexdigest()==digest,'Source changed: '+name
  info=zipfile.ZipInfo('velocimd/'+name);info.create_system=3;info.external_attr=(p.stat().st_mode&0xffff)<<16
  z.writestr(info,data,compress_type=zipfile.ZIP_DEFLATED,compresslevel=9)
`
  execFileSync(process.env.PYTHON||'python3',['-c',script,root,output],{input:JSON.stringify(source),encoding:'utf8',timeout:30000})
}
