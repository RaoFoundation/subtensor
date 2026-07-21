# bittensor-website

Next.js app (Fumadocs) serving bittensor.com/docs. Part of the `website/` Yarn
workspaces + Turbo monorepo — see `website/README.md` for install and build
commands.

## Docs (`/docs`)

The documentation content lives in the repo-root `docs/` folder (Fumadocs;
see `source.config.ts`), built to be the agentic lookup surface for doing
anything on Bittensor. That folder is the single source of truth for all docs:
user-facing concepts and guides, the generated reference, and runtime/contributor
internals (`docs/internals/`).

The reference section (`docs/tx`, `docs/query`, `docs/errors`) and the
JSON catalogs (`public/catalog/`) are **generated** from the SDK's own
registries — never edit them by hand:

```bash
# run from sdk/python, whose environment provides the bittensor SDK
uv run python ../../website/apps/bittensor-website/scripts/generate.py            # regenerate
uv run python ../../website/apps/bittensor-website/scripts/generate.py --check    # CI drift gate
```

Everything else under `docs/` is hand-written.

Agent-facing endpoints: `/llms.txt` (curated index + search tips; omits per-op
reference listings), `/llms-full.txt` (full prose dump for offline `rg`/RAG),
raw markdown for every page under `/llms.mdx/docs/<slug>/content.md`, the
catalogs at `/catalog/{intents,reads,errors}.json`, and the chain Rust source
at `/code` (plain text at `/code/raw/<path>`, machine-readable index at
`/code/index.json`). See `docs/agents.mdx` § Searching these docs.

## Development

From the `website/` directory:

```bash
yarn
npx turbo run dev --filter=@raofoundation/bittensor-website...
```

Open [http://localhost:3000](http://localhost:3000) to see the app.
