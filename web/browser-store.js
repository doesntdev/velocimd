// The browser-only host adapter. No timer, worker, native filesystem or fallback
// writer. Rust admits one patch at a time; close may admit one final latest patch.
export const KEY = 'velocimd.browser-drafts.v1';
export const DB_NAME = 'velocimd.browser-drafts.v2';
export const MARKER_VERSION = 2;
const LIMIT = 4 * 1024 * 1024;
const bytes = s => new TextEncoder().encode(s).length;
const checksum = async raw => [...new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(raw)))].map(b=>b.toString(16).padStart(2,'0')).join('');
const same = (a,b) => JSON.stringify(a) === JSON.stringify(b);
const check = (condition,message) => { if(!condition)throw new Error(message); };
const exact = (value,keys) => value && typeof value==='object' && !Array.isArray(value) && same(Object.keys(value).sort(),keys.slice().sort());
const nameOK = s => typeof s==='string' && bytes(s)>0 && bytes(s)<=160 && !/[\p{Cc}\\/:*?"<>|]/u.test(s) && /\.(md|markdown)$/i.test(s) && s.slice(0,s.lastIndexOf('.')).trim().length>0;
export const logicalBytes = s => bytes(JSON.stringify(s));
export function validateLegacy(raw) {
  check(typeof raw==='string' && bytes(raw)<=LIMIT,'Stored drafts exceed the 4 MiB recovery limit');
  let s;try{s=JSON.parse(raw);}catch{throw new Error('Stored drafts are malformed or from an unsupported format');}
  check(exact(s,['version','documents','active','mode','split_ratio','light']) && s.version===1 &&
    Array.isArray(s.documents) && s.documents.length>0 && s.documents.length<=16 &&
    Number.isInteger(s.active) && s.active>=0 && s.active<s.documents.length &&
    ['Edit','Preview','Split'].includes(s.mode) && typeof s.light==='boolean' &&
    Number.isFinite(s.split_ratio) && s.split_ratio>=0.15 && s.split_ratio<=0.85 &&
    s.documents.every(d=>exact(d,['title','content']) && nameOK(d.title) && typeof d.content==='string'),
    'Stored drafts are malformed or from an unsupported format');
  return s;
}
function openDB() {
  return new Promise((resolve,reject) => {
    const r=indexedDB.open(DB_NAME,1);
    r.onupgradeneeded=()=>{for(const n of ['session','documents','recovery'])r.result.createObjectStore(n);};
    r.onerror=()=>reject(r.error);
    r.onblocked=()=>reject(new Error('Browser storage upgrade blocked; close other editors'));
    r.onsuccess=()=>{const db=r.result;db.onversionchange=()=>db.close();resolve(db);};
  });
}
// Every operation resolves only on transaction completion, not request success.
// Keep IDB callbacks synchronous: awaiting a promise in a request handler could
// let the transaction auto-commit before all body/metadata requests are admitted.
function transaction(db,mode,run,stores=['session','documents']) {
  return new Promise((resolve,reject) => {
    let tx, value, failure;
    try {
      tx=db.transaction(stores,mode,mode==='readwrite'?{durability:'strict'}:undefined);
      tx.oncomplete=()=>resolve(value);
      tx.onabort=()=>reject(failure || tx.error || new Error('Draft transaction aborted'));
      tx.onerror=()=>{};
      const fail=e=>{failure=e;try{tx.abort();}catch{reject(e);}};
      run(tx,v=>{value=v;},fail);
    }catch(e){if(tx){try{tx.abort();}catch{}}reject(e);}
  });
}
function request(r,fn,fail) {r.onsuccess=()=>{try{fn(r.result);}catch(e){fail(e);}};}
async function snapshot(db) {
  return transaction(db,'readonly',(tx,done,fail)=>{
    let head, docs, keys, migration, n=0;
    const finish=()=>{if(++n===4)done({head:head??null,documents:keys.map((k,i)=>[k,docs[i]]),migration:migration??null});};
    const s=tx.objectStore('session'), d=tx.objectStore('documents');
    request(s.get('head'),v=>{head=v;finish();},fail);
    request(s.get('migration'),v=>{migration=v;finish();},fail);
    request(d.getAllKeys(),v=>{keys=v;finish();},fail);
    request(d.getAll(),v=>{docs=v;finish();},fail);
  });
}
async function unpack(snap) {
  check(typeof snap.head==='string' && bytes(snap.head)<=65536 && snap.documents.length<=16,'Stored drafts are malformed or from an unsupported format');
  const head=JSON.parse(snap.head);
  check(exact(head,['version','generation','sequence','documents','active','mode','split_ratio','light']) && head.version===2 &&
    typeof head.generation==='string' && Number.isSafeInteger(head.sequence) && head.sequence>0 && Array.isArray(head.documents) && head.documents.length>0 && head.documents.length<=16,
    'Stored drafts are malformed or from an unsupported format');
  const map=new Map(snap.documents), seen=new Set();
  const docs=await Promise.all(head.documents.map(async d=>{
    check(exact(d,['id','title','revision','body_bytes','checksum']) && typeof d.id==='string' && /^[1-9][0-9]{0,14}$/.test(d.id) && !seen.has(d.id) &&
      Number.isSafeInteger(d.revision) && d.revision>=0 && Number.isSafeInteger(d.body_bytes) && d.body_bytes>=2,
      'Stored document metadata is corrupt');
    seen.add(d.id);const raw=map.get(d.id);
    check(typeof raw==='string' && bytes(raw)===d.body_bytes && typeof d.checksum==='string' && await checksum(raw)===d.checksum,'Stored document body is missing or corrupt');
    const content=JSON.parse(raw);check(typeof content==='string','Stored document body is corrupt');
    return {title:d.title,content};
  }));
  check(map.size===seen.size,'Stored drafts contain unreferenced bodies');
  const raw=JSON.stringify({version:1,documents:docs,active:head.active,mode:head.mode,split_ratio:head.split_ratio,light:head.light});
  validateLegacy(raw);
  // The Rust decoder is authoritative for numeric/Unicode/serde rules too.
  // Do this before open() can authorize any writes or finish a version marker.
  globalThis.velocimdValidateDrafts(raw);
  return {head,raw};
}
export class BrowserDraftStore {
  constructor() {this.tail=Promise.resolve();this.closing=false;this.expected=new Map();this.head=null;this.legacy=null;this.blocked=false;this.recovery=null;}
  async open() {
    let raw=null, descriptors=[];
    try {
      this.legacy=localStorage.getItem(KEY);
      this.db=await openDB();
      this.original=await snapshot(this.db);
      this.head=this.original.head;
      this.expected=new Map(this.original.documents);
      check(this.head!==null || this.original.documents.length===0 && this.original.migration===null,'Stored drafts are corrupt: orphaned document or migration records');
      if(this.head!==null) {
        const restored=await unpack(this.original);raw=restored.raw;descriptors=restored.head.documents;
        this.generation=restored.head.generation;
        this.marker=JSON.stringify({version:MARKER_VERSION,store:DB_NAME,generation:this.generation});
        const migration=this.original.migration;
        if(this.legacy!==this.marker) {
          // Interrupted migration may only finish its marker at the EXACT
          // preserved legacy baseline, before any subsequent save is admitted.
          check(migration && migration.head===this.head && migration.legacy===this.legacy,
            'Drafts changed in another browser tab or version; stored bytes preserved');
          await this.finishMigration();
        }
      } else if(this.legacy!==null) {
        globalThis.velocimdValidateDrafts(this.legacy);
        const s=validateLegacy(this.legacy);raw=this.legacy;
        this.generation=crypto.randomUUID();
        this.marker=JSON.stringify({version:MARKER_VERSION,store:DB_NAME,generation:this.generation});
        descriptors=await Promise.all(s.documents.map(async(d,i)=>({id:String(i+1),title:d.title,revision:0,body_bytes:bytes(JSON.stringify(d.content)),checksum:await checksum(JSON.stringify(d.content))})));
        const head=JSON.stringify({version:2,generation:this.generation,sequence:1,documents:descriptors,active:s.active,mode:s.mode,split_ratio:s.split_ratio,light:s.light});
        const bodies=s.documents.map((d,i)=>[String(i+1),JSON.stringify(d.content)]);
        await this.write(head,bodies,{legacy:this.legacy,head});
        // Readback includes every legacy byte in the recovery/migration record.
        await this.finishMigration();
      } else {
        this.generation=crypto.randomUUID();
        this.marker=JSON.stringify({version:MARKER_VERSION,store:DB_NAME,generation:this.generation});
      }
      return JSON.stringify({raw,descriptors,durable:true,error:null,recovery:null});
    } catch(e) {
      this.blocked=true;
      // A valid restored session remains usable even if final marker write failed.
      try { this.recovery=this.original && (this.original.head!==null || this.original.documents.length || this.original.migration!==null) ? JSON.stringify({legacy:this.legacy,store:this.original}) : this.legacy; } catch { this.recovery=null; }
      return JSON.stringify({raw,descriptors,durable:false,error:this.error(e),recovery:this.recovery});
    }
  }
  error(e) {return `${e?.message || e} (browser storage unavailable, denied, quota full or transaction aborted). Notes remain in memory; download before leaving.`;}
  checkLegacy() {check(localStorage.getItem(KEY)===this.legacy,'Drafts changed in another browser tab or version; reload after downloading current notes');}
  async finishMigration() {
    this.checkLegacy();
    const snap=await snapshot(this.db);
    check(snap.head===this.head && snap.migration?.head===this.head && snap.migration?.legacy===this.legacy && same(snap.documents,[...this.expected].sort((a,b)=>a[0].localeCompare(b[0]))),
      'Migration verification failed; original bytes preserved');
    // The verification above awaited IDB. Recheck immediately before the
    // synchronous marker replacement; never overwrite an intervening v1 write.
    this.checkLegacy();
    localStorage.setItem(KEY,this.marker);
    check(localStorage.getItem(KEY)===this.marker,'Migration marker readback failed; original bytes preserved');
    this.legacy=this.marker;
    this.blocked=false;this.recovery=null;
  }
  // CAS the head plus ONLY overwritten/deleted body baselines in the SAME
  // readwrite transaction as all body changes and coherent metadata.
  async write(head,bodies,migration=null) {
    this.checkLegacy();
    const next=JSON.parse(head), ids=new Set(next.documents.map(d=>d.id));
    const removed=[...this.expected.keys()].filter(id=>!ids.has(id));
    const changes=new Map(bodies);
    const affected=[...new Set([...changes.keys(),...removed])];
    const previous=this.head;
    await transaction(this.db,'readwrite',(tx,done,fail)=>{
      const s=tx.objectStore('session'), d=tx.objectStore('documents');
      request(s.get('head'),v=>{
        check((v??null)===previous,'Draft store changed outside this editor; original bytes preserved');
        let remaining=affected.length;
        const commit=()=>{
          this.checkLegacy();
          for(const [id,body] of changes)d.put(body,id);
          for(const id of removed)d.delete(id);
          if(migration) {
            s.put(migration,'migration');
            tx.objectStore('recovery').put(migration.legacy,'legacy:'+next.generation);
          }
          s.put(head,'head');done(null);
        };
        if(!remaining)commit();
        for(const id of affected)request(d.get(id),v=>{
          check(same(v,this.expected.get(id)),'Draft body changed outside this editor; original bytes preserved');
          if(--remaining===0)commit();
        },fail);
      },fail);
    },migration?['session','documents','recovery']:['session','documents']);
    // Update expected state on COMMIT even if verification fails; Retry still
    // compares this exact head, never resets to an outside writer's baseline.
    this.head=head;
    for(const [id,body] of changes)this.expected.set(id,body);
    for(const id of removed)this.expected.delete(id);
    await transaction(this.db,'readonly',(tx,done,fail)=>{
      const s=tx.objectStore('session'), d=tx.objectStore('documents');
      request(s.get('head'),v=>{check(v===head,'Draft readback mismatch');done(null);},fail);
      for(const [id,body] of changes)request(d.get(id),v=>check(v===body,'Draft body readback mismatch'),fail);
      for(const id of removed)request(d.get(id),v=>check(v===undefined,'Draft deletion readback mismatch'),fail);
      if(migration)request(tx.objectStore('recovery').get('legacy:'+next.generation),v=>check(v===migration.legacy,'Legacy recovery readback mismatch'),fail);
    },migration?['session','documents','recovery']:['session','documents']);
    this.checkLegacy();
  }
  admit(raw,replace=false) {
    if(this.closing)return Promise.resolve(JSON.stringify({error:'Editor is closing; download before leaving',recovery:this.recovery}));
    const task=this.tail.then(()=>this.save(raw,replace));
    this.tail=task.catch(()=>{});
    return task;
  }
  async save(raw,replace) {
    try {
      if(!this.db) {
        // Startup read-denied can retry ONLY if storage is still empty. Never
        // adopt drafts that arrived while the editor was operating in memory.
        check(localStorage.getItem(KEY)===null && this.head===null && this.legacy===null,'Stored drafts changed; reload after downloading current notes');
        this.db=await openDB();const snap=await snapshot(this.db);
        check(snap.head===null && snap.documents.length===0 && snap.migration===null,'Draft store changed; reload after downloading current notes');
        this.generation=crypto.randomUUID();this.marker=JSON.stringify({version:MARKER_VERSION,store:DB_NAME,generation:this.generation});this.blocked=false;
      }
      if(replace)await this.preserveRecovery();
      else if(this.blocked) {
        const snap=await snapshot(this.db);
        check(snap.migration?.head===this.head && snap.migration?.legacy===this.legacy,'Draft recovery required; download stored recovery before replacing');
        await this.finishMigration();
      }
      this.checkLegacy();
      const patch=JSON.parse(raw);
      // Rust provides validated descriptors and exact logical-size accounting.
      check(Number.isSafeInteger(patch.logical_bytes) && patch.logical_bytes<=LIMIT,'Drafts exceed 4 MiB; download your notes, then close some tabs');
      const prior=this.head===null || this.replacing ? null : JSON.parse(this.head);
      const sequence=prior===null ? 1 : prior.sequence+1;
      const changed=new Map(patch.bodies);
      for(const d of patch.meta.documents) {
        d.checksum=changed.has(d.id)?await checksum(changed.get(d.id)):prior?.documents.find(old=>old.id===d.id)?.checksum;
        check(typeof d.checksum==='string','Draft body checksum is missing; download before leaving');
      }
      const head=JSON.stringify({version:2,generation:this.generation,sequence,...patch.meta});
      // Fresh origins need the same fail-closed old-client marker as migrations.
      if(this.legacy!==this.marker) {
        const migration={legacy:this.legacy,head};
        await this.write(head,patch.bodies,migration);
        await this.finishMigration();
      } else await this.write(head,patch.bodies);
      this.blocked=false;this.recovery=null;this.replacing=false;
      return JSON.stringify({error:null,recovery:null});
    }catch(e) {
      // Preserve the original recovery baseline. Retry must NOT refresh/adopt it.
      if(!this.recovery && /changed|mismatch|recovery/.test(String(e))) {
        this.blocked=true;
        try{this.recovery=JSON.stringify({legacy:localStorage.getItem(KEY),store:await snapshot(this.db)});}catch{}
      }
      return JSON.stringify({error:this.error(e),recovery:this.recovery});
    }
  }
  async preserveRecovery() {
    check(this.recovery!==null && this.db,'No stored recovery to replace');this.checkLegacy();
    const snap=await snapshot(this.db);
    // After outside change, the editor's acknowledged baseline remains protected.
    // A reload is required to establish a recovery replacement baseline.
    check(same(snap,this.original),'Stored drafts changed again; reload after downloading current notes');
    const backup=JSON.stringify({legacy:this.legacy,store:snap}), key=crypto.randomUUID();
    await transaction(this.db,'readwrite',(tx,done)=>{tx.objectStore('recovery').put(backup,key);done(null);},['recovery']);
    await transaction(this.db,'readonly',(tx,done,fail)=>request(tx.objectStore('recovery').get(key),v=>{check(v===backup,'Recovery backup mismatch; original left intact');done(null);},fail),['recovery']);
    this.checkLegacy();check(same(await snapshot(this.db),snap),'Stored drafts changed again; original bytes preserved');
    // Invalid head is not parsed by save; only after a verified backup may the
    // replacement be constructed. The write CAS still uses the invalid old head.
    this.replacing=true;this.blocked=false;
    this.generation=crypto.randomUUID();this.marker=JSON.stringify({version:MARKER_VERSION,store:DB_NAME,generation:this.generation});
  }
  close() {this.closing=true;return this.tail.finally(()=>this.db?.close());}
}
