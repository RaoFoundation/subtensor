import {createMDX} from 'fumadocs-mdx/next';

const withMDX = createMDX();

/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,

  async redirects() {
    return [
      // spec.bittensor.com: the Polkadot Vault onboarding shortcut — lands on
      // the guide section hosting the one-time chain-specs QR.
      {
        source: '/:path*',
        has: [{type: 'host', value: 'spec.bittensor.com'}],
        destination: 'https://www.bittensor.com/docs/guides/vault',
        permanent: false,
      },
      {
        source: '/documentation/:path*',
        destination: '/docs',
        permanent: true,
      },
      // v432 and v434 were squashed into the v431 and v435 release pages.
      {
        source: '/releases/v432-upgrade',
        destination: '/releases/v431-upgrade',
        permanent: true,
      },
      {
        source: '/releases/v434-upgrade',
        destination: '/releases/v435-upgrade',
        permanent: true,
      },
      {
        source: '/scan/:path*',
        destination: 'https://taostats.io',
        permanent: true,
      },
      {
        source: '/scan',
        destination: 'https://taostats.io',
        permanent: true,
      },
    ];
  },

  transpilePackages: ['@raofoundation/ui'],

  webpack(config) {
    config.module.rules.push({
      test: /\.glsl/,
      type: 'asset/source',
    });

    config.externals.push({
      'utf-8-validate': 'commonjs utf-8-validate',
      bufferutil: 'commonjs bufferutil',
    });

    // Grab the existing rule that handles SVG imports
    const fileLoaderRule = config.module.rules.find((rule) => rule.test?.test?.('.svg'));

    config.module.rules.push(
      // Reapply the existing rule, but only for svg imports ending in ?url
      {
        ...fileLoaderRule,
        test: /\.svg$/i,
        resourceQuery: /url/, // *.svg?url
      },
      // Convert all other *.svg imports to React components
      {
        test: /\.svg$/i,
        issuer: fileLoaderRule.issuer,
        resourceQuery: {not: [...fileLoaderRule.resourceQuery.not, /url/]}, // exclude if *.svg?url
        use: ['@svgr/webpack'],
      },
    );

    // Modify the file loader rule to ignore *.svg, since we have it handled now.
    fileLoaderRule.exclude = /\.svg$/i;

    return config;
  },
};

export default withMDX(nextConfig);
