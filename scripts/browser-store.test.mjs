import test from 'node:test';
import assert from 'node:assert/strict';
import {validateLegacy, logicalBytes, MARKER_VERSION} from '../web/browser-store.js';
const session = () => ({version:1,documents:[{title:'Exact.MD',content:'\ufeffcafé 🦀\r\n'}],active:0,mode:'Split',split_ratio:0.5,light:false});
test('legacy validation preserves exact Unicode and rejects untrusted native/session fields', () => {
  const s=session(); assert.deepEqual(validateLegacy(JSON.stringify(s)),s);
  for(const update of [{version:2},{active:2},{split_ratio:0},{documents:[]},{path:'/private'},{documents:[{title:'../bad.md',content:''}]}])assert.throws(()=>validateLegacy(JSON.stringify({...s,...update})));
  assert.equal(MARKER_VERSION,2);
});
test('logical session accounting includes JSON escapes and UTF-8, not just body lengths', () => {
  const s=session();assert.equal(logicalBytes(s),Buffer.byteLength(JSON.stringify(s)));
  s.documents[0].content='\u0000'.repeat(700000);assert.throws(()=>validateLegacy(JSON.stringify(s)),/4 MiB/);
});
