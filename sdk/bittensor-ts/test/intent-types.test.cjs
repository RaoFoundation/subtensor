const test = require('node:test')
const assert = require('node:assert/strict')
const path = require('node:path')
const ts = require('typescript')

test('generated intent builders require typed arguments, including recursive child specs', () => {
  const program = ts.createProgram([path.join(__dirname, 'types/intents.ts')], {
    noEmit: true, strict: true, skipLibCheck: true, target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.CommonJS, moduleResolution: ts.ModuleResolutionKind.Node10,
    esModuleInterop: true,
  })
  const errors = ts.getPreEmitDiagnostics(program)
  assert.equal(errors.length, 0, ts.formatDiagnosticsWithColorAndContext(errors, {
    getCanonicalFileName: name => name, getCurrentDirectory: () => process.cwd(), getNewLine: () => '\n',
  }))
})
