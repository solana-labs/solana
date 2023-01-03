import alias from '@rollup/plugin-alias';
import babel from '@rollup/plugin-babel';
import commonjs from '@rollup/plugin-commonjs';
import * as fs from 'fs';
import path from 'path';
import nodeResolve from '@rollup/plugin-node-resolve';
import replace from '@rollup/plugin-replace';
import {terser} from 'rollup-plugin-terser';

const env = process.env.NODE_ENV;
const extensions = ['.js', '.ts'];

function generateConfig(configType, format) {
  const browser = configType === 'browser' || configType === 'react-native';
  const bundle = format === 'iife';

  const config = {
    input: 'src/index.ts',
    plugins: [
      alias({
        entries: [
          {
            find: /^\./, // Relative paths.
            replacement: '.',
            async customResolver(source, importer, options) {
              const resolved = await this.resolve(source, importer, {
                skipSelf: true,
                ...options,
              });
              if (resolved == null) {
                return;
              }
              const {id: resolvedId} = resolved;
              const directory = path.dirname(resolvedId);
              const moduleFilename = path.basename(resolvedId);
              const forkPath = path.join(
                directory,
                '__forks__',
                configType,
                moduleFilename,
              );
              const hasForkCacheKey = `has_fork:${forkPath}`;
              let hasFork = this.cache.get(hasForkCacheKey);
              if (hasFork === undefined) {
                hasFork = fs.existsSync(forkPath);
                this.cache.set(hasForkCacheKey, hasFork);
              }
              if (hasFork) {
                return forkPath;
              }
            },
          },
        ],
      }),
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
          'process.env.npm_package_version': JSON.stringify(
            process.env.npm_package_version,
          ),
        },
      }),
    ],
    onwarn: function (warning, rollupWarn) {
      rollupWarn(warning);
      if (warning.code === 'CIRCULAR_DEPENDENCY') {
        throw new Error(
          'Please eliminate the circular dependencies listed ' +
            'above and retry the build',
        );
      }
    },
    treeshake: {
      moduleSideEffects: false,
    },
  };

  if (!browser) {
    // Prevent dependencies from being bundled
    config.external = [
      /@babel\/runtime/,
      '@noble/hashes/hmac',
      '@noble/hashes/sha256',
      '@noble/hashes/sha3',
      '@noble/hashes/sha512',
      '@noble/ed25519',
      '@noble/secp256k1',
      '@solana/buffer-layout',
      'bigint-buffer',
      'bn.js',
      'borsh',
      'bs58',
      'buffer',
      'crypto-hash',
      'jayson/lib/client/browser',
      'node-fetch',
      'rpc-websockets',
      'superstruct',
    ];
  }

  switch (configType) {
    case 'browser':
    case 'react-native':
      switch (format) {
        case 'iife': {
          config.external = ['http', 'https', 'node-fetch'];

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
        default: {
          config.output = [
            {
              file: `lib/index.${
                configType === 'react-native' ? 'native' : 'browser.cjs'
              }.js`,
              format: 'cjs',
              sourcemap: true,
            },
            configType === 'browser'
              ? {
                  file: 'lib/index.browser.esm.js',
                  format: 'es',
                  sourcemap: true,
                }
              : null,
          ].filter(Boolean);

          // Prevent dependencies from being bundled
          config.external = [
            /@babel\/runtime/,
            '@solana/buffer-layout',
            '@noble/hashes/hmac',
            '@noble/hashes/sha256',
            '@noble/hashes/sha3',
            '@noble/hashes/sha512',
            '@noble/ed25519',
            '@noble/secp256k1',
            'bigint-buffer',
            'bn.js',
            'borsh',
            'bs58',
            'buffer',
            'crypto-hash',
            'http',
            'https',
            'jayson/lib/client/browser',
            'node-fetch',
            'react-native-url-polyfill',
            'rpc-websockets',
            'superstruct',
          ];

          break;
        }
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
      break;
    default:
      throw new Error(`Unknown configType: ${configType}`);
  }

  return config;
}

export default [
  generateConfig('node'),
  generateConfig('browser'),
  generateConfig('browser', 'iife'),
  generateConfig('react-native'),
];
