// Browser artifact format v1. Producer checks include complete patched dependency source.
// Trust comes from the explicitly approved archive/manifest pin, not a self-signed receipt.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { readFile, lstat, mkdir, mkdtemp, rename, rm, writeFile, open, unlink } from 'node:fs/promises'
import { join, dirname, resolve, basename } from 'node:path'
import { RUNTIME_ASSETS, shaPattern, validateVerification } from './parity-proof.mjs'

export const DISTRIBUTION_FILES = Object.freeze([...RUNTIME_ASSETS, 'source.zip', 'build.json'])
export const sha = bytes => createHash('sha256').update(bytes).digest('hex')
export function safeSourcePath(name) {
  return typeof name === 'string' && name.length > 0 && !name.startsWith('/') && !name.includes('\\') &&
    ![...name].some(character => character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127) && !name.includes(':') && !name.split('/').some(part => !part || part === '.' || part === '..' || part === '.git' || /^\.env(?:\.|$)/.test(part)) &&
    !/(?:^|\/)(?:node_modules|__pycache__)(?:\/|$)/.test(name) && !/^(?:target|graphify-out)(?:\/|$)/.test(name) && !name.endsWith('.pyc')
}

const sourceArchiveCheck = `import sys,json,zipfile,hashlib,stat,tomllib
m=json.load(sys.stdin)
with zipfile.ZipFile(sys.argv[1]) as z:
 infos=z.infolist(); names=[i.filename for i in infos]
 expected={'velocimd/'+n:h for n,h in m['source']['files'].items()}
 assert len(names)==len(set(names)), 'Duplicate source archive entry'
 assert sorted(names)==sorted(expected), 'Source archive member set mismatch'
 assert sum(i.file_size for i in infos)<=67108864, 'Source archive exceeds bound'
 assert len(infos)<=10000, 'Too many source entries'
 for i in infos:
  assert not i.is_dir() and not stat.S_ISLNK(i.external_attr>>16), 'Nonregular source entry'
  assert hashlib.sha256(z.read(i)).hexdigest()==expected[i.filename], 'Source archive hash mismatch: '+i.filename
 cargo=tomllib.loads(z.read('velocimd/Cargo.toml').decode())
 lock=tomllib.loads(z.read('velocimd/Cargo.lock').decode())
 if cargo.get('patch',{}).get('crates-io',{}).get('epaint',{}).get('path')=='vendor/epaint-0.34.1':
  prefix='velocimd/vendor/epaint-0.34.1/'
  upstream=json.loads(z.read(prefix+'UPSTREAM.json'))
  for name in [*upstream['files'], *upstream['supplemental_upstream_licenses']]:
   assert prefix+name in expected, 'Missing modified dependency source: '+name
 contract=json.loads(z.read('velocimd/tests/parity/contract.json'))
 packages={p['name']:p['version'] for p in lock['package'] if p['name'] in ['pulldown-cmark','wasm-bindgen','web-time','egui','eframe']}
 print(json.dumps({'version':cargo['package']['version'],'contract':contract,'rustPackages':packages}))
`

export async function validateDistribution(directory, {pin, requirePin=false}={}) {
  const folder = resolve(directory)
  const manifestInfo = await lstat(join(folder, 'build.json'))
  assert(manifestInfo.isFile() && !manifestInfo.isSymbolicLink() && manifestInfo.size > 0 && manifestInfo.size <= 4194304, 'Nonregular/oversized browser manifest')
  const manifestBytes = await readFile(join(folder, 'build.json'))
  const manifest = JSON.parse(manifestBytes)
  assert.equal(manifest.format, 1, 'Unsupported browser distribution format')
  assert.equal(manifest.license, 'AGPL-3.0-only', 'Wrong/missing source license')
  assert(typeof manifest.source?.baseCommit==='string' && /^[0-9a-f]{40}$/.test(manifest.source.baseCommit), 'Invalid source commit')
  assert.equal(typeof manifest.source.modifiedBrowserBuild, 'boolean', 'Missing source dirty-state declaration')
  assert(manifest.source.files && !Array.isArray(manifest.source.files), 'Missing corresponding source inventory')
  const sourceNames=Object.keys(manifest.source.files)
  assert(sourceNames.length>0 && sourceNames.every(safeSourcePath), 'Unsafe source member')
  assert(sourceNames.every(name=>typeof manifest.source.files[name]==='string'&&shaPattern.test(manifest.source.files[name])), 'Invalid source hash')
  assert.equal(sha(JSON.stringify(manifest.source.files)), manifest.source.sha256, 'Source inventory digest mismatch')
  for (const name of ['LICENSE','Cargo.toml','Cargo.lock','src/lib.rs','src/ui.rs','src/ui/browser.rs','web/browser-store.js','scripts/build-web.sh','tests/parity/contract.json']) {
    assert(Object.hasOwn(manifest.source.files,name), `Missing corresponding source: ${name}`)
  }
  assert.deepEqual(Object.keys(manifest.files).sort(), [...RUNTIME_ASSETS,'source.zip'].sort(), 'Wrong browser asset set')
  const bytesByName = new Map()
  for (const [name,record] of Object.entries(manifest.files)) {
    const info=await lstat(join(folder,name))
    assert(info.isFile() && !info.isSymbolicLink(), `Nonregular browser asset: ${name}`)
    assert(Number.isSafeInteger(record.bytes) && record.bytes>0 && record.bytes<=67108864, `Invalid browser asset bound: ${name}`)
    assert(shaPattern.test(record.sha256), `Invalid asset hash: ${name}`)
    const bytes=await readFile(join(folder,name))
    assert.equal(bytes.length,record.bytes,`Browser asset size mismatch: ${name}`)
    assert.equal(sha(bytes),record.sha256,`Browser asset hash mismatch: ${name}`)
    bytesByName.set(name,bytes)
  }
  const wasm=bytesByName.get('velocimd_bg.wasm')
  assert.deepEqual([...wasm.subarray(0,4)],[0,97,115,109],'Not a WASM module')
  const exports=new Set(WebAssembly.Module.exports(new WebAssembly.Module(wasm)).map(item=>item.name))
  for (const name of ['render_markdown','webhandle_new','webhandle_start','webhandle_snapshot','webhandle_active_document','webhandle_ui_snapshot','webhandle_set_ui_probe']) {
    assert(exports.has(name), `Missing actual app WASM export: ${name}`)
  }
  const sourceMeta=JSON.parse(execFileSync(process.env.PYTHON || 'python3',['-c',sourceArchiveCheck,join(folder,'source.zip')],{
    input:JSON.stringify(manifest),encoding:'utf8',maxBuffer:2*1024*1024,timeout:30000,
  }))
  assert.equal(manifest.version,sourceMeta.version,'Cargo/browser version mismatch')
  assert.deepEqual(manifest.rustPackages,sourceMeta.rustPackages,'Locked Rust package metadata differs')
  const assets=Object.fromEntries(RUNTIME_ASSETS.map(name=>[name,manifest.files[name].sha256]))
  validateVerification(manifest.verification,{sourceSha256:manifest.source.sha256,assets,contract:sourceMeta.contract})
  if (requirePin) assert(pin,'Missing approved browser artifact pin')
  if (pin) {
    assert.equal(pin.schema,1,'Unsupported artifact pin')
    assert(shaPattern.test(pin.archiveSha256),'Invalid archive pin')
    assert.equal(pin.manifestSha256,sha(manifestBytes),'Manifest differs from approved pin')
    assert.equal(pin.sourceSha256,manifest.source.sha256,'Source differs from approved pin')
    assert.equal(pin.baseCommit,manifest.source.baseCommit,'Source commit differs from approved pin')
  }
  return {manifest,manifestSha256:sha(manifestBytes),contract:sourceMeta.contract}
}

const extractDistribution = `import sys,zipfile,stat,pathlib,io
expected={'index.html','velocimd.js','velocimd_bg.wasm','browser-store.js','source.zip','build.json'}
with zipfile.ZipFile(io.BytesIO(sys.stdin.buffer.read(67108865))) as z:
 infos=z.infolist();names=[i.filename for i in infos]
 assert len(names)==len(set(names)) and set(names)==expected, 'Unsafe/incomplete web archive members'
 assert sum(i.file_size for i in infos)<=67108864, 'Web archive exceeds bound'
 for i in infos:
  assert not i.is_dir() and not stat.S_ISLNK(i.external_attr>>16), 'Nonregular web archive member'
  p=pathlib.Path(sys.argv[1])/i.filename
  with p.open('xb') as f:f.write(z.read(i))
`

export async function importDistribution(options) {
  assert(typeof options?.expectedSha256==='string'&&shaPattern.test(options.expectedSha256),'An exact lowercase SHA-256 archive pin is required')
  const base=join(dirname(resolve(options.pinPath)),'.run/browser-import')
  await mkdir(base,{recursive:true})
  const lockPath=join(base,'import.lock')
  const lock=await open(lockPath,'wx') // Never share a target/pin transaction with another importer.
  const owned=await lock.stat()
  try {
    await lock.writeFile(JSON.stringify({pid:process.pid,archiveSha256:options.expectedSha256})+'\n')
    return await installDistribution(options)
  } finally {
    await lock.close()
    const current=await lstat(lockPath)
    assert(current.dev===owned.dev&&current.ino===owned.ino,'Import lock identity changed; retained for reconciliation')
    await unlink(lockPath)
  }
}

async function installDistribution({archive,expectedSha256,target,pinPath}) {
  assert(typeof expectedSha256==='string'&&shaPattern.test(expectedSha256),'An exact lowercase SHA-256 archive pin is required')
  const archivePath=resolve(archive), destination=resolve(target), approvedPin=resolve(pinPath)
  assert(archivePath!==approvedPin && archivePath!==destination,'Input/output collision')
  const info=await lstat(archivePath)
  assert(info.isFile() && !info.isSymbolicLink() && info.size<=67108864,'Invalid archive input')
  const archiveBytes=await readFile(archivePath)
  assert(archiveBytes.length>0&&archiveBytes.length<=67108864,'Invalid archive snapshot bound')
  assert.equal(sha(archiveBytes),expectedSha256,'Archive differs from approved SHA-256')
  const base=join(dirname(approvedPin),'.run/browser-import')
  await mkdir(base,{recursive:true})
  await mkdir(dirname(destination),{recursive:true})
  const directoryInfo=await lstat(dirname(destination))
  assert(directoryInfo.isDirectory() && !directoryInfo.isSymbolicLink(),'Refuse symlink/non-directory parent')
  assert.equal((await lstat(base)).dev,directoryInfo.dev,'Import staging and target must share a filesystem')
  const stage=await mkdtemp(join(base,'transaction-'))
  const ready=join(stage,'ready'), backup=join(stage,'previous'), pinBefore=join(stage,'previous-pin.json'), pinNext=join(stage,'next-pin.json')
  await mkdir(ready)
  let priorBundle=false, priorPin=false, installed=false, pinInstalled=false, settled=false
  try {
    // Feed the exact approved bytes; never reopen a mutable caller pathname.
    execFileSync(process.env.PYTHON || 'python3',['-c',extractDistribution,ready],{input:archiveBytes,encoding:'utf8',timeout:30000})
    const checked=await validateDistribution(ready)
    const pin={schema:1,archiveSha256:expectedSha256,manifestSha256:checked.manifestSha256,
      sourceSha256:checked.manifest.source.sha256,baseCommit:checked.manifest.source.baseCommit,
      artifact:basename(archivePath),kind:'verified-import'}
    try {
      const old=await lstat(approvedPin)
      assert(old.isFile() && !old.isSymbolicLink(),'Refuse replacing a nonregular pin')
      await writeFile(pinBefore,await readFile(approvedPin),{flag:'wx'})
      priorPin=true
    } catch(error){if(error.code!=='ENOENT')throw error}
    await writeFile(pinNext,JSON.stringify(pin,null,2)+'\n',{flag:'wx'})
    await validateDistribution(ready,{pin,requirePin:true})
    try {
      const old=await lstat(destination)
      assert(old.isDirectory() && !old.isSymbolicLink(),'Refuse replacing a non-directory/symlink')
      await rename(destination,backup)
      priorBundle=true
    } catch(error){if(error.code!=='ENOENT')throw error}
    try {
      await rename(ready,destination)
      installed=true
      await rename(pinNext,approvedPin)
      pinInstalled=true
      await validateDistribution(destination,{pin:JSON.parse(await readFile(approvedPin,'utf8')),requirePin:true})
    } catch(error) {
      try {
        if(installed)await rm(destination,{recursive:true})
        if(priorBundle)await rename(backup,destination)
        if(pinInstalled) {
          if(priorPin)await rename(pinBefore,approvedPin)
          else await rm(approvedPin)
        }
        settled=true
      } catch(recoveryError) {
        throw new AggregateError([error,recoveryError],`Import rollback needs recovery from ${stage}: ${recoveryError.message}`,{cause:recoveryError})
      }
      throw error
    }
    settled=true
    return pin
  } finally {
    // Before installing, rejection has not changed either accepted target.
    // Keep the transaction directory if rollback itself failed; never erase its recoverable predecessor.
    if(settled || !priorBundle && !installed)await rm(stage,{recursive:true,force:true})
  }
}
