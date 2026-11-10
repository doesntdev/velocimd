import test from 'node:test';
import assert from 'node:assert/strict';
import {resolve} from 'node:path';
import {readFile} from 'node:fs/promises';
import {inventorySource,isSource} from './source.mjs';
import {RUNTIME_ASSETS} from '../parity-proof.mjs';
import {sha,DISTRIBUTION_FILES} from '../browser-artifact.mjs';
test('browser storage runtime is mandatory in runtime distribution and exact corresponding source inventory',async()=>{
  const root=resolve(import.meta.dirname,'../..');
  const source=await inventorySource(root);
  assert(RUNTIME_ASSETS.includes('browser-store.js'));
  assert(DISTRIBUTION_FILES.includes('browser-store.js'));
  assert(isSource('web/browser-store.js'));
  const prefix='vendor/epaint-0.34.1/';
  const upstream=JSON.parse(await readFile(resolve(root,prefix+'UPSTREAM.json'),'utf8'));
  for(const name of [...Object.keys(upstream.files),...Object.keys(upstream.supplemental_upstream_licenses)]) {
    assert(isSource(prefix+name));
    assert.equal(source.files[prefix+name],sha(await readFile(resolve(root,prefix+name))));
  }
  assert.equal(source.files['web/browser-store.js'],sha(await readFile(resolve(root,'web/browser-store.js'))));
  assert.equal(source.files['src/browser_persistence.rs'],sha(await readFile(resolve(root,'src/browser_persistence.rs'))));
  assert(!Object.keys(source.files).some(name=>name.startsWith('target/')||name.startsWith('node_modules/')));
});
