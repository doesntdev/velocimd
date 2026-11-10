import {writeFile} from 'node:fs/promises'
import {resolve,dirname} from 'node:path'
import {fileURLToPath} from 'node:url'
import {inventorySource} from './source.mjs'
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..')
if(!process.argv[2])throw new Error('Specify a new source record path')
const source=await inventorySource(root)
await writeFile(resolve(process.argv[2]),JSON.stringify(source,null,2)+'\n',{flag:'wx'})
console.log('Source candidate:',source.sha256,'commit:',source.baseCommit,'modified:',source.modifiedBrowserBuild)
