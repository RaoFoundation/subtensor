#!/usr/bin/env node
'use strict'

const fs = require('node:fs')
const path = require('node:path')
const ts = require('typescript')

const root = path.resolve(__dirname, '..')
const sdkRoot = path.resolve(root, '..')
const repoRoot = path.resolve(sdkRoot, '..')
const manifestPath = path.join(sdkRoot, 'bittensor-core', 'binding-manifest.json')
const nativeGeneratedPath = path.join(root, 'native.generated.d.ts')
const nativeDocumentedPath = path.join(root, 'src', 'native.ts')
const browserPath = path.join(root, 'src', 'browser.ts')
const wasmGeneratedPath = path.join(root, 'dist', 'wasm', 'bittensor_core_wasm.d.ts')

function repoRelative(filePath) {
  return path.relative(repoRoot, filePath)
}

function parse(filePath, hint) {
  if (!fs.existsSync(filePath)) {
    throw new Error(`missing ${repoRelative(filePath)}; ${hint}`)
  }
  return ts.createSourceFile(
    filePath,
    fs.readFileSync(filePath, 'utf8'),
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  )
}

function hasModifier(node, kind) {
  return node.modifiers?.some((modifier) => modifier.kind === kind) ?? false
}

function memberName(member) {
  const name = member.name
  if (name == null) return null
  if (ts.isIdentifier(name) || ts.isStringLiteral(name) || ts.isNumericLiteral(name)) {
    return name.text
  }
  return null
}

function declarationNames(statement) {
  if (!ts.isVariableStatement(statement)) return []
  const names = []
  for (const declaration of statement.declarationList.declarations) {
    if (ts.isIdentifier(declaration.name)) names.push(declaration.name.text)
  }
  return names
}

function interfaceMembers(source) {
  const interfaces = new Map()
  for (const statement of source.statements) {
    if (!ts.isInterfaceDeclaration(statement)) continue
    interfaces.set(
      statement.name.text,
      new Set(statement.members.map(memberName).filter((name) => name != null)),
    )
  }
  return interfaces
}

function exportedSurface(source, options = {}) {
  const values = new Set()
  const classes = new Map()
  const ignoredClassMembers = options.ignoredClassMembers ?? new Set()
  const publicOnly = options.publicOnly ?? false

  for (const statement of source.statements) {
    if (!hasModifier(statement, ts.SyntaxKind.ExportKeyword)) continue
    if (ts.isFunctionDeclaration(statement) && statement.name != null) {
      values.add(statement.name.text)
    } else if (ts.isClassDeclaration(statement) && statement.name != null) {
      const className = statement.name.text
      values.add(className)
      const instance = new Set()
      const statics = new Set()
      for (const member of statement.members) {
        if (ts.isConstructorDeclaration(member)) continue
        if (
          publicOnly &&
          (hasModifier(member, ts.SyntaxKind.PrivateKeyword) ||
            hasModifier(member, ts.SyntaxKind.ProtectedKeyword))
        ) {
          continue
        }
        const name = memberName(member)
        if (name == null || ignoredClassMembers.has(name)) continue
        if (hasModifier(member, ts.SyntaxKind.StaticKeyword)) statics.add(name)
        else instance.add(name)
      }
      classes.set(className, { instance, statics })
    } else if (ts.isEnumDeclaration(statement)) {
      values.add(statement.name.text)
    } else {
      for (const name of declarationNames(statement)) values.add(name)
    }
  }

  return { values, classes }
}

function sorted(values) {
  return [...values].sort()
}

function uniqueSet(values, label) {
  if (!Array.isArray(values)) throw new Error(`${label} must be an array`)
  const out = new Set()
  for (const value of values) {
    if (typeof value !== 'string') throw new Error(`${label} entries must be strings`)
    if (out.has(value)) throw new Error(`${label} contains duplicate entry ${value}`)
    out.add(value)
  }
  return out
}

function manifestSurface(manifest, sectionName) {
  const section = manifest[sectionName]
  if (section == null || typeof section !== 'object') {
    throw new Error(`binding manifest is missing ${sectionName}`)
  }

  const values = uniqueSet(section.values ?? [], `${sectionName}.values`)
  const classes = new Map()
  const manifestClasses = section.classes ?? {}
  if (manifestClasses == null || typeof manifestClasses !== 'object' || Array.isArray(manifestClasses)) {
    throw new Error(`${sectionName}.classes must be an object`)
  }

  for (const [className, members] of Object.entries(manifestClasses)) {
    if (members == null || typeof members !== 'object' || Array.isArray(members)) {
      throw new Error(`${sectionName}.classes.${className} must be an object`)
    }
    const instance = uniqueSet(members.instance ?? [], `${sectionName}.classes.${className}.instance`)
    const statics = uniqueSet(members.statics ?? [], `${sectionName}.classes.${className}.statics`)
    values.add(className)
    classes.set(className, { instance, statics })
  }

  return { values, classes }
}

function readManifest() {
  if (!fs.existsSync(manifestPath)) {
    throw new Error(`missing ${repoRelative(manifestPath)}`)
  }
  const raw = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))
  return {
    native: manifestSurface(raw, 'native'),
    wasm: manifestSurface(raw, 'wasm'),
    browser: manifestSurface(raw, 'browser'),
  }
}

function compareSet(label, actual, expected, actualLabel, expectedLabel) {
  const missing = sorted([...expected].filter((name) => !actual.has(name)))
  const stale = sorted([...actual].filter((name) => !expected.has(name)))
  if (missing.length === 0 && stale.length === 0) return []
  const lines = [`${label} differs:`]
  if (missing.length > 0) lines.push(`  missing from ${actualLabel}: ${missing.join(', ')}`)
  if (stale.length > 0) lines.push(`  not in ${expectedLabel}: ${stale.join(', ')}`)
  return lines
}

function compareSurface(label, actual, expected, actualLabel = 'binding', expectedLabel = 'manifest') {
  const failures = compareSet(
    `${label} top-level exports`,
    actual.values,
    expected.values,
    actualLabel,
    expectedLabel,
  )

  for (const [className, expectedMembers] of expected.classes) {
    const actualMembers = actual.classes.get(className)
    if (actualMembers == null) {
      failures.push(`${label} is missing class ${className}`)
      continue
    }
    failures.push(
      ...compareSet(
        `${label}.${className} instance members`,
        actualMembers.instance,
        expectedMembers.instance,
        actualLabel,
        expectedLabel,
      ),
      ...compareSet(
        `${label}.${className} static members`,
        actualMembers.statics,
        expectedMembers.statics,
        actualLabel,
        expectedLabel,
      ),
    )
  }

  return failures
}

function compareClassInterfaces(label, interfaces, expected, mapping) {
  const failures = []
  for (const [className, expectedMembers] of expected.classes) {
    const interfaceNames = mapping[className]
    if (interfaceNames == null) {
      failures.push(`${label} has no interface mapping for ${className}`)
      continue
    }

    const instance = interfaces.get(interfaceNames.instance)
    if (instance == null) {
      failures.push(`${label} is missing interface ${interfaceNames.instance} for ${className}`)
    } else {
      failures.push(
        ...compareSet(
          `${label}.${className} instance members`,
          instance,
          expectedMembers.instance,
          'interface',
          'manifest',
        ),
      )
    }

    if (interfaceNames.statics == null && expectedMembers.statics.size > 0) {
      failures.push(`${label} has no static interface mapping for ${className}`)
    } else if (interfaceNames.statics != null) {
      const statics = interfaces.get(interfaceNames.statics)
      if (statics == null) {
        failures.push(`${label} is missing interface ${interfaceNames.statics} for ${className} statics`)
        continue
      }
      failures.push(
        ...compareSet(
          `${label}.${className} static members`,
          statics,
          expectedMembers.statics,
          'interface',
          'manifest',
        ),
      )
    }
  }
  return failures
}

function cloneSurface(surface) {
  return {
    values: new Set(surface.values),
    classes: new Map(
      [...surface.classes].map(([className, members]) => [
        className,
        {
          instance: new Set(members.instance),
          statics: new Set(members.statics),
        },
      ]),
    ),
  }
}

const nativeClassInterfaces = {
  NativeKeypair: { instance: 'NativeKeypairHandle' },
  NativeRuntime: { instance: 'NativeRuntimeHandle', statics: 'NativeRuntimeConstructor' },
  NativeCursor: { instance: 'NativeCursorHandle', statics: 'NativeCursorConstructor' },
  NativeLedgerDevice: { instance: 'NativeLedgerHandle', statics: 'NativeLedgerConstructor' },
}

const wasmClassInterfaces = {
  Keypair: { instance: 'BrowserWasmKeypair', statics: 'BrowserWasmKeypairConstructor' },
  Runtime: { instance: 'BrowserWasmRuntime', statics: 'BrowserWasmRuntimeConstructor' },
}

try {
  const manifest = readManifest()
  const nativeGenerated = exportedSurface(
    parse(nativeGeneratedPath, 'run npm run build:native first'),
  )
  const nativeSource = parse(nativeDocumentedPath, 'restore src/native.ts')
  const nativeDocumented = interfaceMembers(nativeSource)
  const nativeBinding = nativeDocumented.get('NativeBinding')
  if (nativeBinding == null) throw new Error('src/native.ts does not declare NativeBinding')

  const wasmGenerated = exportedSurface(
    parse(wasmGeneratedPath, 'run npm run build:wasm first'),
    { ignoredClassMembers: new Set(['free']) },
  )
  const browserSource = parse(browserPath, 'restore src/browser.ts')
  const browserInterfaces = interfaceMembers(browserSource)
  const browserPublic = exportedSurface(browserSource, { publicOnly: true })
  const browserModule = browserInterfaces.get('BrowserWasmModule')
  if (browserModule == null) throw new Error('src/browser.ts does not declare BrowserWasmModule')
  const expectedBrowserModule = cloneSurface(manifest.wasm)
  expectedBrowserModule.values.add('default')

  const failures = [
    ...compareSurface('native N-API generated declarations', nativeGenerated, manifest.native),
    ...compareSet(
      'src/native.ts NativeBinding',
      nativeBinding,
      manifest.native.values,
      'interface',
      'manifest',
    ),
    ...compareClassInterfaces('src/native.ts class handles', nativeDocumented, manifest.native, nativeClassInterfaces),
    ...compareSurface('browser WASM generated declarations', wasmGenerated, manifest.wasm),
    ...compareSet(
      'src/browser.ts BrowserWasmModule',
      browserModule,
      expectedBrowserModule.values,
      'interface',
      'manifest',
    ),
    ...compareClassInterfaces('src/browser.ts WASM class handles', browserInterfaces, manifest.wasm, wasmClassInterfaces),
    ...compareSurface(
      'src/browser.ts public browser wrapper',
      browserPublic,
      manifest.browser,
      'wrapper',
      'manifest',
    ),
  ]

  if (failures.length > 0) {
    console.error('Bittensor core binding parity check failed:')
    for (const failure of failures) console.error(failure)
    process.exit(1)
  }
  console.log(
    `Binding parity OK: native ${manifest.native.values.size} exports, ` +
      `WASM ${manifest.wasm.values.size} exports, browser ${manifest.browser.values.size} exports`,
  )
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error))
  process.exit(1)
}
