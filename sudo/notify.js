#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const { notifyProposal } = require("./lib/notify");

async function main() {
  const [file] = process.argv.slice(2);
  if (!file || process.argv.length !== 3) throw new Error("Usage: node sudo/notify.js PROPOSAL.json");
  // Do not emit the secret even inside an add-mask command: command output
  // can bypass Actions' masking. Neither this CLI nor the library logs it.
  const proposal = JSON.parse(fs.readFileSync(file, "utf8"));
  const result = await notifyProposal({ proposal, env: process.env });
  return result.status;
}

if (require.main === module) {
  main().catch((error) => {
    // The library deliberately returns sanitized errors. Do not print an
    // exception object, request configuration, environment, or stack here.
    console.error(`::error::Discord notification failed: ${error.message}`);
    process.exitCode = 1;
  });
}

module.exports = { main };
