import alias from '@rollup/plugin-alias';
import babel from '@rollup/plugin-babel';
import commonjs from '@rollup/plugin-commonjs';
import json from '@rollup/plugin-json';
import multi from '@rollup/plugin-multi-entry';
import nodeResolve from '@rollup/plugin-node-resolve';
import nodePolyfills from 'rollup-plugin-node-polyfills';
import replace from '@rollup/plugin-replace';

const extensions = ['.js', '.ts'];

export default {
  input: {
    include: ['test/**/*.test.ts'],
    exclude: ['test/agent-manager.test.ts', 'test/bpf-loader.test.ts'],
  },
  external: ['node-forge', 'http2', '_stream_wrap'],
  output: {
    file: 'test/dist/bundle.js',
    format: 'es',
    sourcemap: true,
  },
  plugins: [
    multi(),
    commonjs(),
    nodeResolve({
      browser: true,
      preferBuiltins: false,
      extensions,
      dedupe: ['bn.js', 'buffer'],
    }),
    babel({
      exclude: '**/node_modules/**',
      extensions,
      babelHelpers: 'runtime',
      plugins: ['@babel/plugin-transform-runtime'],
    }),
    nodePolyfills(),
    replace({
      'process.env.BROWSER': 'true',
      'process.env.TEST_LIVE': 'true',
    }),
    alias({
      entries: [
        {
          find: /^\.\.\/src\/.*\.js$/,
          replacement: './lib/index.browser.esm.js',
        },
      ],
    }),
    json(),
  ],
  onwarn: function (warning, rollupWarn) {
    if (warning.code !== 'CIRCULAR_DEPENDENCY' && warning.code !== 'EVAL') {
      rollupWarn(warning);
    }
  },
  treeshake: {
    moduleSideEffects: path => path.endsWith('test.ts'),
  },
};
