#!/usr/bin/env node
'use strict'

const fs = require('node:fs')
const path = require('node:path')
const ts = require('typescript')

const root = path.resolve(__dirname, '..')
const generatedPath = path.join(root, 'native.generated.d.ts')
const documentedPath = path.join(root, 'src', 'native.ts')

function parse(filePath) {
  if (!fs.existsSync(filePath)) {
    throw new Error(`missing ${path.relative(root, filePath)}; run npm run build:native first`)
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

function generatedSurface(source) {
  const values = new Set()
  const classes = new Map()
  for (const statement of source.statements) {
    if (!hasModifier(statement, ts.SyntaxKind.ExportKeyword)) continue
    if (ts.isFunctionDeclaration(statement) && statement.name != null) {
      values.add(statement.name.text)
    } else if (ts.isClassDeclaration(statement) && statement.name != null) {
      const name = statement.name.text
      values.add(name)
      const instance = new Set()
      const statics = new Set()
      for (const member of statement.members) {
        if (ts.isConstructorDeclaration(member)) continue
        const name = memberName(member)
        if (name == null) continue
        if (hasModifier(member, ts.SyntaxKind.StaticKeyword)) statics.add(name)
        else instance.add(name)
      }
      classes.set(name, { instance, statics })
    } else if (ts.isEnumDeclaration(statement)) {
      values.add(statement.name.text)
    } else {
      for (const name of declarationNames(statement)) values.add(name)
    }
  }
  return { values, classes }
}

function documentedBinding(source) {
  const interfaces = interfaceMembers(source)
  const binding = interfaces.get('NativeBinding')
  if (binding == null) throw new Error('src/native.ts does not declare NativeBinding')
  return { interfaces, binding }
}

function sorted(values) {
  return [...values].sort()
}

function compareSet(label, actual, expected) {
  const missing = sorted([...expected].filter((name) => !actual.has(name)))
  const stale = sorted([...actual].filter((name) => !expected.has(name)))
  if (missing.length === 0 && stale.length === 0) return []
  const lines = [`${label} differs:`]
  if (missing.length > 0) lines.push(`  missing documentation: ${missing.join(', ')}`)
  if (stale.length > 0) lines.push(`  stale documentation: ${stale.join(', ')}`)
  return lines
}

const classInterfaces = {
  NativeKeypair: { instance: 'NativeKeypairHandle' },
  NativeRuntime: { instance: 'NativeRuntimeHandle', statics: 'NativeRuntimeConstructor' },
  NativeCursor: { instance: 'NativeCursorHandle', statics: 'NativeCursorConstructor' },
  NativeLedgerDevice: { instance: 'NativeLedgerHandle', statics: 'NativeLedgerConstructor' },
}

try {
  const generated = generatedSurface(parse(generatedPath))
  const documented = documentedBinding(parse(documentedPath))
  const failures = compareSet('top-level native exports', documented.binding, generated.values)

  for (const [className, members] of generated.classes) {
    const mapping = classInterfaces[className]
    if (mapping == null) {
      failures.push(`generated class ${className} has no documented interface mapping`)
      continue
    }
    const instance = documented.interfaces.get(mapping.instance)
    if (instance == null) {
      failures.push(`missing interface ${mapping.instance} for ${className}`)
    } else {
      failures.push(...compareSet(`${className} instance members`, instance, members.instance))
    }

    const statics = mapping.statics == null ? new Set() : documented.interfaces.get(mapping.statics)
    if (statics == null) {
      failures.push(`missing interface ${mapping.statics} for ${className} statics`)
    } else {
      failures.push(...compareSet(`${className} static members`, statics, members.statics))
    }
  }

  if (failures.length > 0) {
    console.error('Native Rust/TypeScript parity check failed:')
    for (const failure of failures) console.error(failure)
    process.exit(1)
  }
  console.log(`Native parity OK: ${generated.values.size} exports and ${generated.classes.size} classes`)
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error))
  process.exit(1)
}
