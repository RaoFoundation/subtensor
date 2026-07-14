module.exports = {
  plugins: [
    'postcss-import',
    'postcss-flexbugs-fixes',
    'postcss-nested',
    'postcss-extend-rule',
    'postcss-mixins',
    'postcss-simple-vars',
    [
      'postcss-preset-env',
      {
        autoprefixer: {
          flexbox: 'no-2009',
        },
        stage: 3,
        features: {
          'custom-properties': false,
        },
      },
    ],
    'cssnano',
  ],
}
