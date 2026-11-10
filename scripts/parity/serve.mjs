import { createServer } from 'node:http'
import { readFile } from 'node:fs/promises'
import { join } from 'node:path'

export async function serveWeb(folder) {
  const types={'index.html':'text/html; charset=utf-8','velocimd.js':'text/javascript; charset=utf-8','velocimd_bg.wasm':'application/wasm','source.zip':'application/zip','build.json':'application/json'}
  const server=createServer(async(request,response)=>{
    const name=new URL(request.url,'http://127.0.0.1').pathname.slice(1)||'index.html'
    if(name==='favicon.ico'){response.writeHead(204);response.end();return}
    if(!Object.hasOwn(types,name)){response.writeHead(404);response.end('Not found');return}
    try {
      const bytes=await readFile(join(folder,name))
      response.writeHead(200,{'Content-Type':types[name],'Cache-Control':'no-store','X-Content-Type-Options':'nosniff'})
      response.end(bytes)
    } catch(error){response.writeHead(error.code==='ENOENT'?404:500);response.end('Unavailable')}
  })
  await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',resolve)})
  const address=server.address()
  return {url:`http://127.0.0.1:${address.port}/index.html`,close:()=>new Promise((resolve,reject)=>server.close(error=>error?reject(error):resolve()))}
}
