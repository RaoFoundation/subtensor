#!/bin/bash

set -Eeuo pipefail

# React Native build fails if we use default pnpm hoisting strategy
# Production build on Vercel fails if we won't
# So we need to use override the way pnpm instll dependencies on Vercel

rm .npmrc
rm pnpm-lock.yaml
mv prod.pnpm-lock.yaml pnpm-lock.yaml
