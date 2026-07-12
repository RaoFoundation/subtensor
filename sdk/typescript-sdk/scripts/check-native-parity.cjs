#!/usr/bin/env node
'use strict'

const fs = require('node:fs')
const path = require('node:path')
const ts = require('typescript')

const root = path.resolve(__dirname, '..')
const sdkRoot = path.resolve(root, '..')
const repoRoot = path.resolve(sdkRoot, '..')
const nativeRustRoot = path.join(root, 'native', 'src')
const wasmRustRoot = path.join(sdkRoot, 'bittensor-core-wasm', 'src')
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

function readRustFiles(directory) {
  if (!fs.existsSync(directory)) throw new Error(`missing ${repoRelative(directory)}`)
  const out = []
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const item = path.join(directory, entry.name)
    if (entry.isDirectory()) out.push(...readRustFiles(item))
    else if (entry.isFile() && entry.name.endsWith('.rs')) out.push(item)
  }
  return out.sort()
}

function snakeToCamel(name) {
  return name.replace(/_([a-zA-Z0-9])/g, (_, value) => value.toUpperCase())
}

function jsNameFrom(attrs, rustName, mode) {
  for (const attr of attrs) {
    const match = attr.match(/\bjs_name\s*=\s*(?:"([^"]+)"|([A-Za-z0-9_]+))/)
    if (match != null) return match[1] ?? match[2]
  }
  return mode === 'napi' ? snakeToCamel(rustName) : rustName
}

function hasAttrFlag(attrs, flag) {
  return attrs.some((attr) => new RegExp(`\\b${flag}\\b`).test(attr))
}

function hasBindingAttr(attrs, mode) {
  return attrs.some((attr) => attr.startsWith(mode))
}

function ensureClass(surface, className) {
  surface.values.add(className)
  let members = surface.classes.get(className)
  if (members == null) {
    members = { instance: new Set(), statics: new Set() }
    surface.classes.set(className, members)
  }
  return members
}

function rustBindingSurface(directory, mode) {
  const surface = { values: new Set(), classes: new Map() }
  for (const filePath of readRustFiles(directory)) {
    const source = fs.readFileSync(filePath, 'utf8')
    parseRustBindingSource(source, mode, surface)
  }
  return surface
}

function parseRustBindingSource(source, mode, surface) {
  const lines = source.split(/\r?\n/)
  const stack = []
  let pendingAttrs = []
  let pendingImpl = null

  for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
    const rawLine = lines[lineIndex]
    const line = rawLine.trim()
    if (line.startsWith('#[')) {
      const attr = line.replace(/^#\[/, '').replace(/\]\s*$/, '')
      if (attr.startsWith(mode)) pendingAttrs.push(attr)
      updateRustStack(rawLine, stack)
      continue
    }

    const implStart = line.match(/^impl\s+([A-Za-z0-9_]+)/)
    if (implStart != null && line.includes('{')) {
      pendingImpl = {
        name: implStart[1],
        binding: hasBindingAttr(pendingAttrs, mode),
      }
    }

    const classMatch = line.match(/^pub\s+(?:struct|enum)\s+([A-Za-z0-9_]+)/)
    if (classMatch != null && hasBindingAttr(pendingAttrs, mode) && !hasAttrFlag(pendingAttrs, 'object')) {
      if (line.startsWith('pub enum ')) surface.values.add(classMatch[1])
      else ensureClass(surface, classMatch[1])
      pendingAttrs = []
    }

    const functionMatch = line.match(/^pub\s+fn\s+([A-Za-z0-9_]+)/)
    const implInfo = currentRustImpl(stack)
    const hasFunctionBinding =
      hasBindingAttr(pendingAttrs, mode) || (mode === 'wasm_bindgen' && implInfo?.binding)
    if (functionMatch != null && hasFunctionBinding) {
      const rustName = functionMatch[1]
      if (!hasAttrFlag(pendingAttrs, 'constructor')) {
        const name = jsNameFrom(pendingAttrs, rustName, mode)
        if (implInfo == null) {
          surface.values.add(name)
        } else {
          const classMembers = ensureClass(surface, implInfo.name)
          const signature = rustFunctionSignature(lines, lineIndex)
          const isStatic = hasAttrFlag(pendingAttrs, 'factory') || !/\bself\b/.test(signature)
          ;(isStatic ? classMembers.statics : classMembers.instance).add(name)
        }
      }
      pendingAttrs = []
    } else if (line.length > 0 && !line.startsWith('#') && !line.startsWith('//')) {
      pendingAttrs = []
    }

    updateRustStack(rawLine, stack, pendingImpl)
    pendingImpl = null
  }
}

function currentRustImpl(stack) {
  for (let index = stack.length - 1; index >= 0; index -= 1) {
    if (stack[index].kind === 'impl') return stack[index]
  }
  return null
}

function updateRustStack(rawLine, stack, pendingImpl = null) {
  for (const char of rawLine) {
    if (char === '{') {
      if (pendingImpl != null) stack.push({ kind: 'impl', ...pendingImpl })
      else stack.push({ kind: 'block' })
      pendingImpl = null
    } else if (char === '}') {
      stack.pop()
    }
  }
}

function rustFunctionSignature(lines, startIndex) {
  const parts = []
  for (let index = startIndex; index < lines.length; index += 1) {
    const line = lines[index]
    parts.push(line)
    if (line.includes(')')) break
  }
  const text = parts.join('\n')
  const start = text.indexOf('(')
  const end = text.indexOf(')', Math.max(0, start))
  return start >= 0 && end >= 0 ? text.slice(start + 1, end) : ''
}

function sorted(values) {
  return [...values].sort()
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

function compareSurface(label, actual, expected, actualLabel = 'binding', expectedLabel = 'expected surface') {
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

function compareClassInterfaces(
  label,
  interfaces,
  expected,
  mapping,
  expectedLabel = 'Rust annotations',
) {
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
          expectedLabel,
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
          expectedLabel,
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

function allowlistedSurface(values, classes = {}) {
  return {
    values: new Set(values),
    classes: new Map(
      Object.entries(classes).map(([className, members]) => {
        const instance = new Set(members.instance ?? [])
        const statics = new Set(members.statics ?? [])
        return [className, { instance, statics }]
      }),
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

const browserWrapperExpected = allowlistedSurface(
  [
    'CRYPTO_ED25519',
    'CRYPTO_SR25519',
    'DEFAULT_SS58_FORMAT',
    'Keypair',
    'Runtime',
    'configureBrowserWasm',
    'coreVersion',
    'decryptWithSignature',
    'encrypt',
    'encryptAtRound',
    'eraBirth',
    'generateCommitV2',
    'generateExtrinsicProof',
    'getEncryptedCommitment',
    'initBrowser',
    'loadBrowser',
    'metadataDigest',
    'mlkemKdfId',
    'multisigAccountId',
    'publicKeyFromSs58',
    'ready',
    'revealRound',
    'sealMevShieldTransaction',
    'setDefaultBrowserWasmLoader',
    'ss58FromPublic',
    'verifySignature',
  ],
  {
    Runtime: {
      instance: [
        'composeCall',
        'constant',
        'decode',
        'decodeBatch',
        'decodeCall',
        'decodeExtrinsic',
        'decodeMapChanges',
        'decodeMapPairs',
        'decodeStorageKeyParams',
        'encode',
        'encodeEra',
        'encodeSignedExtrinsic',
        'extrinsicVersion',
        'isV15',
        'metadataIr',
        'moduleError',
        'registryJson',
        'runtimeApiMap',
        'runtimeApis',
        'signaturePayload',
        'signaturePayloadParts',
        'signedExtensionIdentifiers',
        'specVersion',
        'ss58Format',
        'storageEntry',
        'storageKey',
        'storageKeyBatch',
        'storagePrefix',
        'transactionVersion',
        'typeIdOf',
        'typeNameOf',
      ],
    },
    Keypair: {
      statics: [
        'createFromMnemonic',
        'createFromPrivateKey',
        'createFromSeed',
        'createFromUri',
        'fromMnemonic',
        'fromPrivateKey',
        'fromSeed',
        'fromUri',
        'generateMnemonic',
      ],
      instance: [
        'address',
        'addressRaw',
        'cryptoType',
        'derive',
        'isLocked',
        'kind',
        'meta',
        'publicKey',
        'scheme',
        'setMeta',
        'sign',
        'ss58Address',
        'ss58Format',
        'type',
        'verify',
      ],
    },
  },
)

try {
  const nativeExpected = rustBindingSurface(nativeRustRoot, 'napi')
  const wasmExpected = rustBindingSurface(wasmRustRoot, 'wasm_bindgen')
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
  const expectedBrowserModule = cloneSurface(wasmExpected)
  expectedBrowserModule.values.add('default')

  const failures = [
    ...compareSurface(
      'native N-API generated declarations',
      nativeGenerated,
      nativeExpected,
      'generated declarations',
      'Rust #[napi] annotations',
    ),
    ...compareSet(
      'src/native.ts NativeBinding',
      nativeBinding,
      nativeExpected.values,
      'interface',
      'Rust #[napi] annotations',
    ),
    ...compareClassInterfaces(
      'src/native.ts class handles',
      nativeDocumented,
      nativeExpected,
      nativeClassInterfaces,
    ),
    ...compareSurface(
      'browser WASM generated declarations',
      wasmGenerated,
      wasmExpected,
      'generated declarations',
      'Rust #[wasm_bindgen] annotations',
    ),
    ...compareSet(
      'src/browser.ts BrowserWasmModule',
      browserModule,
      expectedBrowserModule.values,
      'interface',
      'Rust #[wasm_bindgen] annotations',
    ),
    ...compareClassInterfaces(
      'src/browser.ts WASM class handles',
      browserInterfaces,
      wasmExpected,
      wasmClassInterfaces,
    ),
    ...compareSurface(
      'src/browser.ts public browser wrapper',
      browserPublic,
      browserWrapperExpected,
      'wrapper',
      'browser wrapper allowlist',
    ),
  ]

  if (failures.length > 0) {
    console.error('Bittensor core binding parity check failed:')
    for (const failure of failures) console.error(failure)
    process.exit(1)
  }
  console.log(
    `Binding parity OK: native ${nativeExpected.values.size} Rust exports, ` +
      `WASM ${wasmExpected.values.size} Rust exports, browser portable wrapper ${browserWrapperExpected.values.size} exports`,
  )
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error))
  process.exit(1)
}
