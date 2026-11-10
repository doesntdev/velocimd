import test from 'node:test'
import assert from 'node:assert/strict'
import {mkdtemp,rm,writeFile,readFile} from 'node:fs/promises'
import {join} from 'node:path'
import {PNG} from 'pngjs'
import {compareRaster,eraseRegion} from './visual.mjs'

async function fixture(fn){
  assert(process.env.TMPDIR,'Use owned TMPDIR for verifier fixtures')
  const folder=await mkdtemp(join(process.env.TMPDIR,'parity-visual-'))
  try{
    const image=new PNG({width:96,height:72})
    for(let y=0;y<72;y++)for(let x=0;x<96;x++){
      const i=(y*96+x)*4,ink=x>8&&x<86&&y>10&&y<32&&(x%7<4||y%5<2)
      image.data[i]=ink?230:25;image.data[i+1]=ink?210:28;image.data[i+2]=ink?200:32;image.data[i+3]=255
    }
    const original=join(folder,'original.png');await writeFile(original,PNG.sync.write(image))
    await fn(folder,original)
  }finally{await rm(folder,{recursive:true})}
}

test('ordinary visual verifier accepts byte-identical images and emits a real diff',()=>fixture(async(folder,original)=>{
  const diff=join(folder,'diff.png'),before=await readFile(original)
  const result=await compareRaster(original,original,{diffPath:diff})
  assert.equal(result.mismatchPixels,0);assert((await readFile(diff)).length>0)
  assert.deepEqual(await readFile(original),before)
}))
test('erasing actual image content fails the ordinary visual oracle',()=>fixture(async(folder,original)=>{
  const altered=join(folder,'erased.png');await eraseRegion(original,altered,[8,8,80,30])
  await assert.rejects(()=>compareRaster(original,altered),/Visual parity/)
}))
test('a local missing glyph cannot hide in a large mostly matching page',()=>fixture(async(folder,original)=>{
  const altered=join(folder,'missing-glyph.png');await eraseRegion(original,altered,[15,14,16,16])
  await assert.rejects(()=>compareRaster(original,altered),/Visual parity/)
}))
test('wrong dimensions and out-of-bounds masks cannot make a comparison pass',()=>fixture(async(folder,original)=>{
  const tiny=join(folder,'tiny.png');await writeFile(tiny,PNG.sync.write(new PNG({width:10,height:10})))
  await assert.rejects(()=>compareRaster(original,tiny),/width/)
  await assert.rejects(()=>compareRaster(original,original,{masks:[[80,0,30,20]]}),/outside/)
}))
