const isCalledByBundler = caller => caller.name.startsWith('@rollup');

module.exports = api => {
  const calledByBundler = api.caller(isCalledByBundler);
  const transformEnvVariables =
    !calledByBundler &&
    process.env.NODE_ENV &&
    process.env.BROWSER !== undefined;

  return {
    presets: [
      [
        '@babel/preset-env',
        {
          modules: false,
          bugfixes: true,
        },
      ],
      ['@babel/preset-typescript'],
    ],
    plugins: [
      [
        '@babel/plugin-proposal-class-properties',
        {
          loose: true,
        },
      ],
      [
        '@babel/plugin-proposal-private-methods',
        {
          loose: true,
        },
      ],
      [
        '@babel/plugin-proposal-private-property-in-object',
        {
          loose: true,
        },
      ],
      transformEnvVariables && [
        'transform-inline-environment-variables',
        {
          include: ['NODE_ENV', 'BROWSER'],
        },
      ],
      transformEnvVariables && [
        'minify-dead-code-elimination',
        {
          keepFnName: true,
          keepFnArgs: true,
          keepClassName: true,
        },
      ],
    ].filter(Boolean),
  };
};
