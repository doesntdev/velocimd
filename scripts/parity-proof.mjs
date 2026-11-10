import assert from 'node:assert/strict'

export const REQUIRED_GATES = Object.freeze([
  'architecture', 'rust-fmt', 'rust-test', 'rust-clippy', 'native-build', 'wasm-build',
  'native-ui', 'browser-chromium', 'browser-firefox', 'visual-parity', 'negative-controls', 'tooling-tests',
  'browser-regressions-chromium', 'browser-regressions-firefox',
])
export const RUNTIME_ASSETS = Object.freeze(['index.html', 'velocimd.js', 'velocimd_bg.wasm'])
export const shaPattern = /^[0-9a-f]{64}$/

export function validateVerification(report, {sourceSha256, assets, contract}) {
  assert(report && report.schema === 1 && report.status === 'passed', 'Missing/pending/failed parity verification')
  assert(typeof sourceSha256 === 'string' && shaPattern.test(sourceSha256) && report.sourceSha256 === sourceSha256, 'Verification source mismatch')
  assert.deepEqual(report.assets, assets, 'Verification asset identity mismatch')
  assert.deepEqual([...report.matrix?.browsers || []].sort(), ['chromium', 'firefox'], 'Incomplete browser matrix')
  assert.deepEqual([...report.matrix?.caseIds || []].sort(), contract.cases.map(item => item.id).sort(), 'Incomplete fixture matrix')
  assert.deepEqual(report.consoleErrors, [], 'Browser console/startup errors were not clean')
  const journeys=contract.requiredJourneys
  for(const key of ['native','browser','browserRegressions']) {
    const ids=journeys?.[key]
    assert(Array.isArray(ids)&&ids.length>0&&ids.every(id=>typeof id==='string'&&id.length>0)&&new Set(ids).size===ids.length,`Missing/invalid required journey inventory: ${key}`)
  }
  const fixtures=contract.cases.map(item=>item.id)
  const expectedCases=new Map([
    ['native-ui',[...fixtures,...journeys.native]],
    ['visual-parity',['native-baseline','chromium','firefox'].flatMap(target=>fixtures.map(id=>`${target}:${id}`))],
  ])
  for(const engine of ['chromium','firefox']) {
    expectedCases.set('browser-'+engine,[...fixtures,...journeys.browser])
    expectedCases.set('browser-regressions-'+engine,journeys.browserRegressions)
  }
  assert(Array.isArray(report.gates), 'Missing verification gates')
  const gateIds = report.gates.map(gate => gate.id)
  assert.equal(gateIds.length, new Set(gateIds).size, 'Duplicate verification gate')
  for (const id of REQUIRED_GATES) assert(gateIds.includes(id), `Missing required gate: ${id}`)
  for (const gate of report.gates) {
    assert.equal(gate.status, 'passed', `Incomplete gate: ${gate.id}`)
    assert.equal(gate.exitCode, 0, `Nonzero/missing gate exit: ${gate.id}`)
    const substantive = expectedCases.has(gate.id)||gate.id==='negative-controls'
    if (substantive || gate.cases !== undefined || gate.caseCount !== undefined) {
      assert(Array.isArray(gate.cases) && gate.cases.length > 0, `Missing substantive cases: ${gate.id}`)
      assert.equal(gate.caseCount, gate.cases.length, `Wrong declared case total: ${gate.id}`)
      assert.equal(new Set(gate.cases.map(item => item.id)).size, gate.cases.length, `Duplicate cases: ${gate.id}`)
      assert(gate.cases.every(item => typeof item.id === 'string' && item.id.length > 0 && item.status === 'passed'), `Nonpass/invalid case: ${gate.id}`)
      if(expectedCases.has(gate.id)) assert.deepEqual(gate.cases.map(item=>item.id).sort(),[...expectedCases.get(gate.id)].sort(),`Wrong required journey/case inventory: ${gate.id}`)
    }
  }
  return report
}
