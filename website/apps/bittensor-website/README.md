This is a [Next.js](https://nextjs.org/) project bootstrapped with [`create-next-app`](https://github.com/vercel/next.js/tree/canary/packages/create-next-app).

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
# with the bittensor SDK venv active
python scripts/generate.py            # regenerate
python scripts/generate.py --check    # CI drift gate
```

Everything else under `docs/` is hand-written.

Agent-facing endpoints: `/llms.txt`, `/llms-full.txt`, raw markdown for every
page under `/llms.mdx/docs/<slug>/content.md`, and the catalogs at
`/catalog/{intents,reads,errors}.json`.

## Getting Started

First, run the development server:

```bash
npm run dev
# or
yarn dev
# or
pnpm dev
```

Open [http://localhost:3000](http://localhost:3000) with your browser to see the result.

You can start editing the page by modifying `app/page.tsx`. The page auto-updates as you edit the file.

[API routes](https://nextjs.org/docs/api-routes/introduction) can be accessed on [http://localhost:3000/api/hello](http://localhost:3000/api/hello). This endpoint can be edited in `pages/api/hello.ts`.

The `pages/api` directory is mapped to `/api/*`. Files in this directory are treated as [API routes](https://nextjs.org/docs/api-routes/introduction) instead of React pages.

This project uses [`next/font`](https://nextjs.org/docs/basic-features/font-optimization) to automatically optimize and load Inter, a custom Google Font.

## Learn More

To learn more about Next.js, take a look at the following resources:

- [Next.js Documentation](https://nextjs.org/docs) - learn about Next.js features and API.
- [Learn Next.js](https://nextjs.org/learn) - an interactive Next.js tutorial.

You can check out [the Next.js GitHub repository](https://github.com/vercel/next.js/) - your feedback and contributions are welcome!

## Deploy on Vercel

The easiest way to deploy your Next.js app is to use the [Vercel Platform](https://vercel.com/new?utm_medium=default-template&filter=next.js&utm_source=create-next-app&utm_campaign=create-next-app-readme) from the creators of Next.js.

Check out our [Next.js deployment documentation](https://nextjs.org/docs/deployment) for more details.
