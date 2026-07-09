## Apps Monorepo

### Requirements

- Node 20.9+
- Yarn 4 via corepack: `corepack enable` (the version is pinned by `packageManager` in `package.json`)

### Content

##### Apps

| Name                                | Deployment                                                                                       |
| :---------------------------------- | :----------------------------------------------------------------------------------------------- |
| **@raofoundation/bittensor-website**   | Staging on push to `main` (`deploy-docs.yml`); promoted to production by `watch-mainnet-release.yml` |

##### Packages

- **@raofoundation/ui** - shared frontend components
- **eslint-config-custom** - shared ESLint config
- **tsconfig** - shared TypeScript configs

### Development

- Install dependencies with `yarn`
- Run an app in development mode with `npx turbo run dev --filter=@raofoundation/bittensor-website...`
- The three dots at the end of the `--filter=` matter - they tell `turbo` to run all dependencies in `dev` mode too, so changes in a dependent package refresh the whole app

### Deployment

Pushes to `main` that touch `website/**` build and deploy to Vercel staging via the `Deploy Docs` workflow. Production promotion happens automatically in `watch-mainnet-release.yml` once a runtime release executes on chain.
