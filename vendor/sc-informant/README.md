---
title: "Vendored Substrate informant"
description: "Compatibility fix for tracing-subscriber message sanitization."
---

Copied from `substrate/client/informant` in RaoFoundation/polkadot-sdk at
`cacb4310f20c7cac83eb3ccd8ed5a5ad4212608a`.

The only source changes remove `console::style` from log message fields, along
with the unused `console` dependency. This keeps messages readable with stock
`tracing-subscriber 0.3.20` while retaining control-character sanitization and
the logger's timestamp and level colors.
