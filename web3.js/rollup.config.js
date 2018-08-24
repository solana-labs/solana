import babel       from 'rollup-plugin-babel';
import builtins    from 'rollup-plugin-node-builtins';
import commonjs    from 'rollup-plugin-commonjs';
import globals     from 'rollup-plugin-node-globals';
import json        from 'rollup-plugin-json';
import nodeResolve from 'rollup-plugin-node-resolve';
import replace     from 'rollup-plugin-replace';
import uglify      from 'rollup-plugin-uglify';

const env = process.env.NODE_ENV;

function generateConfig(configType) {
  const config = {
    input: 'src/index.js',
    plugins: [
      json(),
      babel({
        exclude: '**/node_modules/**',
        runtimeHelpers: true,
      }),
      replace({
        'process.env.NODE_ENV': JSON.stringify(env),
      }),
      commonjs(),
    ],
  };

  if (env === 'production') {
    config.plugins.push(
      uglify({
        compress: {
          pure_getters: true,
          unsafe: true,
          unsafe_comps: true,
          warnings: false,
        },
      }),
    );
  }

  switch (configType) {
  case 'browser':
    config.output = [
      {
        file: 'lib/index.iife.js',
        format: 'iife',
        name: 'solanaWeb3',
      },
    ];
    config.plugins.push(builtins());
    config.plugins.push(globals());
    config.plugins.push(nodeResolve({
      browser: true,
    }));
    break;
  case 'node':
    config.output = [
      {
        file: 'lib/index.cjs.js',
        format: 'cjs',
      },
      {
        file: 'lib/index.esm.js',
        format: 'es',
      },
    ];
    break;
  default:
    throw new Error(`Unknown configType: ${configType}`);
  }

  return config;
}

export default [
  generateConfig('node'),
  generateConfig('browser'),
];
