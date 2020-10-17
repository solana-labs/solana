import babel from 'rollup-plugin-babel';
import builtins from 'rollup-plugin-node-builtins';
import commonjs from 'rollup-plugin-commonjs';
import copy from 'rollup-plugin-copy';
import globals from 'rollup-plugin-node-globals';
import json from 'rollup-plugin-json';
import nodeResolve from 'rollup-plugin-node-resolve';
import replace from 'rollup-plugin-replace';
import {terser} from 'rollup-plugin-terser';

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
      copy({
        targets: [{src: 'module.d.ts', dest: 'lib', rename: 'index.d.ts'}],
      }),
    ],
  };

  switch (configType) {
    case 'browser':
      config.output = [
        {
          file: 'lib/index.iife.js',
          format: 'iife',
          name: 'solanaWeb3',
          sourcemap: true,
        },
      ];
      config.plugins.push(builtins());
      config.plugins.push(globals());
      config.plugins.push(
        nodeResolve({
          browser: true,
        }),
      );

      if (env === 'production') {
        config.plugins.push(
          terser({
            mangle: false,
            compress: false,
          }),
        );
      }

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

      // Quash 'Unresolved dependencies' complaints for modules listed in the
      // package.json "dependencies" section.  Unfortunately this list is manually
      // maintained.
      config.external = [
        'assert',
        '@babel/runtime/core-js/get-iterator',
        '@babel/runtime/core-js/json/stringify',
        '@babel/runtime/core-js/object/assign',
        '@babel/runtime/core-js/object/get-prototype-of',
        '@babel/runtime/core-js/object/keys',
        '@babel/runtime/core-js/promise',
        '@babel/runtime/helpers/asyncToGenerator',
        '@babel/runtime/helpers/classCallCheck',
        '@babel/runtime/helpers/createClass',
        '@babel/runtime/helpers/defineProperty',
        '@babel/runtime/helpers/get',
        '@babel/runtime/helpers/getPrototypeOf',
        '@babel/runtime/helpers/inherits',
        '@babel/runtime/helpers/possibleConstructorReturn',
        '@babel/runtime/helpers/slicedToArray',
        '@babel/runtime/helpers/toConsumableArray',
        '@babel/runtime/helpers/typeof',
        '@babel/runtime/regenerator',
        'bn.js',
        'bs58',
        'buffer-layout',
        'crypto-hash',
        'http',
        'https',
        'jayson/lib/client/browser',
        'node-fetch',
        'rpc-websockets',
        'superstruct',
        'tweetnacl',
        'url',
        'secp256k1',
        'keccak',
      ];
      break;
    default:
      throw new Error(`Unknown configType: ${configType}`);
  }

  return config;
}

export default [generateConfig('node'), generateConfig('browser')];
