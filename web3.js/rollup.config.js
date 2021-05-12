import babel from '@rollup/plugin-babel';
import commonjs from '@rollup/plugin-commonjs';
import json from '@rollup/plugin-json';
import nodeResolve from '@rollup/plugin-node-resolve';
import nodePolyfills from 'rollup-plugin-node-polyfills';
import replace from '@rollup/plugin-replace';
import {terser} from 'rollup-plugin-terser';

const env = process.env.NODE_ENV;
const extensions = ['.js', '.ts'];

function generateConfig(configType, format) {
  const browser = configType === 'browser';
  const bundle = format === 'iife';

  const config = {
    input: 'src/index.ts',
    plugins: [
      commonjs(),
      nodeResolve({
        browser,
        dedupe: ['bn.js', 'buffer'],
        extensions,
        preferBuiltins: !browser,
      }),
      babel({
        exclude: '**/node_modules/**',
        extensions,
        babelHelpers: bundle ? 'bundled' : 'runtime',
        plugins: bundle ? [] : ['@babel/plugin-transform-runtime'],
      }),
      replace({
        preventAssignment: true,
        values: {
          'process.env.NODE_ENV': JSON.stringify(env),
          'process.env.BROWSER': JSON.stringify(browser),
        },
      }),
    ],
    onwarn: function (warning, rollupWarn) {
      if (warning.code !== 'CIRCULAR_DEPENDENCY') {
        rollupWarn(warning);
      }
    },
    treeshake: {
      moduleSideEffects: false,
    },
  };

  if (configType !== 'browser') {
    // Prevent dependencies from being bundled
    config.external = [
      /@babel\/runtime/,
      'bn.js',
      'bs58',
      'buffer-layout',
      'crypto-hash',
      'jayson/lib/client/browser',
      'js-sha3',
      'node-fetch',
      'rpc-websockets',
      'secp256k1',
      'superstruct',
      'tweetnacl',
    ];
  }

  switch (configType) {
    case 'browser':
      switch (format) {
        case 'esm': {
          config.output = [
            {
              file: 'lib/index.browser.esm.js',
              format: 'es',
              sourcemap: true,
            },
          ];

          // Prevent dependencies from being bundled
          config.external = [
            /@babel\/runtime/,
            'bn.js',
            'bs58',
            'buffer',
            'buffer-layout',
            'crypto-hash',
            'jayson/lib/client/browser',
            'js-sha3',
            'node-fetch',
            'rpc-websockets',
            'secp256k1',
            'superstruct',
            'tweetnacl',
          ];

          break;
        }
        case 'iife': {
          config.output = [
            {
              file: 'lib/index.iife.js',
              format: 'iife',
              name: 'solanaWeb3',
              sourcemap: true,
            },
            {
              file: 'lib/index.iife.min.js',
              format: 'iife',
              name: 'solanaWeb3',
              sourcemap: true,
              plugins: [terser({mangle: false, compress: false})],
            },
          ];

          break;
        }
        default:
          throw new Error(`Unknown format: ${format}`);
      }

      // TODO: Find a workaround to avoid resolving the following JSON file:
      // `node_modules/secp256k1/node_modules/elliptic/package.json`
      config.plugins.push(json());
      config.plugins.push(nodePolyfills());

      break;
    case 'node':
      config.output = [
        {
          file: 'lib/index.cjs.js',
          format: 'cjs',
          sourcemap: true,
        },
        {
          file: 'lib/index.esm.js',
          format: 'es',
          sourcemap: true,
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
  generateConfig('browser', 'esm'),
  generateConfig('browser', 'iife'),
];
