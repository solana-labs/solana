import dts from 'rollup-plugin-dts';

export default {
  input: './declarations/index.d.ts',
  output: [{file: 'lib/index.d.ts', format: 'es'}],
  plugins: [dts()],
};
