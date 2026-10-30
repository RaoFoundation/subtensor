'use strict'

const { rmSync, readdirSync } = require('node:fs')
const { join } = require('node:path')

const root = join(__dirname, '..')
rmSync(join(root, 'dist'), { recursive: true, force: true })
rmSync(join(root, 'native.cjs'), { force: true })
rmSync(join(root, 'native.generated.d.ts'), { force: true })
for (const entry of readdirSync(root)) {
  if (entry.endsWith('.node') || entry.endsWith('.node.map')) {
    rmSync(join(root, entry), { force: true })
  }
}
