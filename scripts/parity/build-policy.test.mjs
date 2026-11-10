import test from 'node:test';
import assert from 'node:assert/strict';
import {validateBuildPolicy} from '../browser-artifact.mjs';
const asset={bytes:100,sha256:'1'.repeat(64)};
const policy=()=>({schema:1,cargoProfile:'release',release:{lto:'thin',codegenUnits:1,optLevel:'3'},rustc:'unit-only compiler',wasmBindgen:'unit-only binding tool',wasmBindgenSha256:'3'.repeat(64),compilerFlagHashes:{RUSTFLAGS:'4'.repeat(64),CARGO_ENCODED_RUSTFLAGS:'4'.repeat(64),CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS:'4'.repeat(64)},wasmPostprocess:{format:1,policy:'optional-name-dwarf-only',before_bytes:110,after_bytes:100,before_sha256:'2'.repeat(64),after_sha256:asset.sha256,removed:[{name:'name',bytes:10}],all_other_section_bytes_preserved:true}});
test('qualified policy binds actual postprocessed asset and balanced removed bytes',()=>assert.equal(validateBuildPolicy(policy(),asset).cargoProfile,'release'));
test('policy rejects changed asset, unbalanced counts or unqualified section removal',()=>{
 for(const mutate of [p=>p.wasmPostprocess.after_sha256='9'.repeat(64),p=>p.wasmPostprocess.after_bytes=99,p=>p.wasmPostprocess.removed[0].bytes=9,p=>p.wasmPostprocess.removed[0].name='producers',p=>p.wasmPostprocess.all_other_section_bytes_preserved=false]){
  const p=policy();mutate(p);assert.throws(()=>validateBuildPolicy(p,asset));
 }
});
test('policy rejects malformed compiler settings and digests',()=>{
 for(const mutate of [p=>p.release.codegenUnits=0,p=>p.release.lto='unqualified',p=>p.wasmBindgenSha256='bad',p=>p.compilerFlagHashes.RUSTFLAGS=null,p=>p.cargoProfile='debug']){
  const p=policy();mutate(p);assert.throws(()=>validateBuildPolicy(p,asset));
 }
});
