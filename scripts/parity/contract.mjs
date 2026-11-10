import assert from 'node:assert/strict'

import { REQUIRED_GATES, validateVerification } from '../parity-proof.mjs'
export { REQUIRED_GATES, validateVerification }

function rect(value, label) {
  assert(Array.isArray(value) && value.length === 4 && value.every(Number.isFinite), `Invalid rect: ${label}`)
  assert(value[2] > 0 && value[3] > 0, `Non-positive rect: ${label}`)
}

export function validateSnapshot(snapshot, contract) {
  assert.equal(snapshot?.schema, 1, 'Missing actual UI snapshot')
  assert.deepEqual(snapshot.viewport, [contract.viewport.width, contract.viewport.height], 'Wrong viewport')
  assert.equal(snapshot.pixels_per_point, contract.viewport.scale, 'Wrong actual scale')
  assert(['Edit', 'Split', 'Preview'].includes(snapshot.mode), 'Invalid mode')
  assert.equal(typeof snapshot.light, 'boolean', 'Missing theme state')
  assert(Number.isFinite(snapshot.split_ratio), 'Invalid split ratio')
  assert.equal(typeof snapshot.active_title, 'string', 'Missing document title')
  assert.equal(typeof snapshot.active_content, 'string', 'Missing document bytes')
  assert(Array.isArray(snapshot.controls), 'Missing control measurements')
  const ids = snapshot.controls.map(item => item.id)
  assert.equal(new Set(ids).size, ids.length, 'Duplicate control ID')
  for (const control of snapshot.controls) {
    rect(control.rect, control.id)
    assert.equal(typeof control.enabled, 'boolean', `Missing availability: ${control.id}`)
    assert.equal(typeof control.selected, 'boolean', `Missing selection: ${control.id}`)
  }
  const toolbar = snapshot.controls.filter(item => item.id.startsWith('toolbar:')).sort((a,b) => a.rect[1] - b.rect[1] || a.rect[0] - b.rect[0])
  assert.deepEqual(toolbar.map(item => item.id), contract.toolbarIds, 'Wrong toolbar inventory/order')
  const regions = ['top_bar', 'command_bar', ...(snapshot.mode === 'Split' ? ['editor', 'preview', 'divider'] : snapshot.mode === 'Edit' ? ['editor'] : ['preview'])]
  for (const id of regions) rect(snapshot.regions?.[id], id)
  return snapshot
}

export function compareGeometry(native, browser, contract) {
  validateSnapshot(native, contract)
  validateSnapshot(browser, contract)
  for (const key of ['mode', 'light', 'active_title', 'active_content', 'pixels_per_point']) {
    assert.deepEqual(browser[key], native[key], `Shared state differs: ${key}`)
  }
  assert(Math.abs(native.split_ratio - browser.split_ratio) <= 0.0001, 'Split ratio differs')
  const tolerance = contract.geometryTolerancePoints
  const compareRect = (a,b,label) => {
    assert(a && b, `Missing geometry rect: ${label}`)
    for (let index=0;index<4;index++) assert(Math.abs(a[index]-b[index]) <= tolerance, `Geometry differs: ${label}[${index}] ${a[index]} != ${b[index]}`)
  }
  const browserControls = new Map(browser.controls.map(item => [item.id, item]))
  const availabilityExceptions = new Set(['toolbar:open-folder', 'toolbar:close'])
  for (const id of contract.toolbarIds) {
    const a = native.controls.find(item => item.id === id), b = browserControls.get(id)
    compareRect(a.rect, b?.rect, id)
    assert.equal(b.selected, a.selected, `Selection differs: ${id}`)
    if (!availabilityExceptions.has(id)) assert.equal(b.enabled, a.enabled, `Availability differs: ${id}`)
    if (id === 'toolbar:open-folder') assert.equal(b.enabled, false, 'Browser working-folder command must declare its unavailable capability')
  }
  if (native.controls.some(item => item.id === 'filename')) {
    compareRect(native.controls.find(item => item.id === 'filename').rect, browserControls.get('filename')?.rect, 'filename')
  }
  for (const [id,a] of Object.entries(native.regions)) {
    if (['top_bar', 'command_bar', 'editor', 'preview', 'divider'].includes(id)) compareRect(a, browser.regions[id], id)
  }
  return {status:'passed', toolbarControls:contract.toolbarIds.length, tolerancePoints:tolerance}
}
