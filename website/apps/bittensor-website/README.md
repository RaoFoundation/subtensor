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

The reference section (`docs/tx`, `docs/query`, `docs/errors.mdx`) and the
JSON catalogs (`public/catalog/`) are **generated** from the SDK's own
registries — never edit them by hand:

```bash
# run from sdk/python, whose environment provides the bittensor SDK
uv run python ../../website/apps/bittensor-website/scripts/generate.py            # regenerate
uv run python ../../website/apps/bittensor-website/scripts/generate.py --check    # CI drift gate
```

Everything else under `docs/` is hand-written.

Agent-facing endpoints: `/llms.txt`, `/llms-full.txt`, raw markdown for every
page under `/llms.mdx/docs/<slug>/content.md`, and the catalogs at
`/catalog/{intents,reads,errors}.json`.

## Development

From the `website/` directory:

```bash
yarn
npx turbo run dev --filter=@raofoundation/bittensor-website...
```

Open [http://localhost:3000](http://localhost:3000) to see the app.
