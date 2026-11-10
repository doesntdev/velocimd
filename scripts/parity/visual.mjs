import assert from 'node:assert/strict'
import { PNG } from 'pngjs'
import pixelmatch from 'pixelmatch'
import { readFile, writeFile } from 'node:fs/promises'

export const VISUAL_POLICY=Object.freeze({colorThreshold:0.12,regionMismatch:0.012,controlMismatch:0.04,tileSize:24,tileMismatch:0.06})

function checkedRect(rect,width,height) {
  assert(Array.isArray(rect)&&rect.length===4&&rect.every(Number.isFinite),'Invalid raster region')
  const [x,y,w,h]=rect.map(Math.round)
  assert(x>=0&&y>=0&&w>0&&h>0&&x+w<=width&&y+h<=height,`Raster rect outside image: ${JSON.stringify(rect)}`)
  return [x,y,w,h]
}
function crop(image,rect) {
  const [x,y,width,height]=checkedRect(rect,image.width,image.height)
  const output=new PNG({width,height})
  PNG.bitblt(image,output,x,y,width,height,0,0)
  return output
}
function mask(image,rect) {
  const [x,y,width,height]=checkedRect(rect,image.width,image.height)
  for(let row=y;row<y+height;row++)for(let col=x;col<x+width;col++){
    const offset=(row*image.width+col)*4
    image.data[offset]=image.data[offset+1]=image.data[offset+2]=0;image.data[offset+3]=255
  }
}

export async function compareRaster(referencePath,currentPath,{diffPath,masks=[],regions,controls=[],policy=VISUAL_POLICY}={}) {
  const reference=PNG.sync.read(await readFile(referencePath)),current=PNG.sync.read(await readFile(currentPath))
  assert.equal(current.width,reference.width,'Raster width differs')
  assert.equal(current.height,reference.height,'Raster height differs')
  for(const rect of masks){mask(reference,rect);mask(current,rect)}
  const diff=new PNG({width:reference.width,height:reference.height})
  const pixels=pixelmatch(reference.data,current.data,diff.data,reference.width,reference.height,{threshold:policy.colorThreshold,includeAA:false,diffMask:true})
  if(diffPath)await writeFile(diffPath,PNG.sync.write(diff))
  const measured=[]
  const sections=regions||{application:[0,0,reference.width,reference.height]}
  for(const [id,rect] of Object.entries(sections)) {
    const a=crop(reference,rect),b=crop(current,rect)
    const count=pixelmatch(a.data,b.data,null,a.width,a.height,{threshold:policy.colorThreshold,includeAA:false})
    const fraction=count/(a.width*a.height)
    measured.push({id,mismatchPixels:count,totalPixels:a.width*a.height,fraction,limit:policy.regionMismatch})
  }
  for(const control of controls) {
    const a=crop(reference,control.rect),b=crop(current,control.rect)
    const count=pixelmatch(a.data,b.data,null,a.width,a.height,{threshold:policy.colorThreshold,includeAA:false})
    measured.push({id:control.id,mismatchPixels:count,totalPixels:a.width*a.height,fraction:count/(a.width*a.height),limit:policy.controlMismatch})
  }
  let worstTile={fraction:0,rect:[0,0,policy.tileSize,policy.tileSize]}
  const stride=Math.max(1,Math.floor(policy.tileSize/2))
  for(let y=0;y<diff.height;y+=stride)for(let x=0;x<diff.width;x+=stride){
    const width=Math.min(policy.tileSize,diff.width-x),height=Math.min(policy.tileSize,diff.height-y)
    let count=0
    for(let row=y;row<y+height;row++)for(let col=x;col<x+width;col++)if(diff.data[(row*diff.width+col)*4+3]!==0)count++
    const fraction=count/(width*height)
    if(fraction>worstTile.fraction)worstTile={fraction,rect:[x,y,width,height]}
  }
  const result={status:'passed',mismatchPixels:pixels,totalPixels:reference.width*reference.height,regions:measured,worstTile,policy}
  const failed=measured.filter(item=>item.fraction>item.limit)
  if(failed.length||worstTile.fraction>policy.tileMismatch){
    const error=new Error(`Visual parity differs: ${failed.map(item=>`${item.id}=${item.fraction.toFixed(5)}`).join(', ')}; worst tile=${worstTile.fraction.toFixed(5)} ${JSON.stringify(worstTile.rect)}`)
    error.metrics={...result,status:'failed'}
    throw error
  }
  return result
}

export async function eraseRegion(input,output,rect) {
  const image=PNG.sync.read(await readFile(input)),[x,y,w,h]=checkedRect(rect,image.width,image.height)
  const sample=(y*image.width+x)*4,color=[...image.data.subarray(sample,sample+4)]
  for(let row=y;row<y+h;row++)for(let col=x;col<x+w;col++)image.data.set(color,(row*image.width+col)*4)
  await writeFile(output,PNG.sync.write(image))
}
