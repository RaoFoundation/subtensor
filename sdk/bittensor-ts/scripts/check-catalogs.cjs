#!/usr/bin/env node
'use strict'
const { execFileSync } = require('node:child_process')
const fs = require('node:fs')
const path = require('node:path')
const sdk = path.resolve(__dirname, '..')
const pythonRoot = path.resolve(sdk, '../python')
const venv = path.join(pythonRoot, '.venv/bin/python')
const python = process.env.PYTHON ?? (fs.existsSync(venv) ? venv : 'python3')
const scripts = ['generate-read-catalog.py', 'generate-intent-catalog.py', 'generate-descriptors.py', 'generate-evm-catalog.py', 'generate-extension-page.py', 'generate-error-catalog.py']
if (fs.existsSync(path.join(__dirname, 'generate-basket-index.py'))) scripts.push('generate-basket-index.py')
for (const script of scripts) execFileSync(python, [path.join(__dirname, script), '--check'], { cwd: pythonRoot, stdio: 'inherit', env: { ...process.env, PYTHONPATH: [pythonRoot, process.env.PYTHONPATH].filter(Boolean).join(path.delimiter) } })
