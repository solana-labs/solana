import typescript from 'rollup-plugin-typescript2';
import rust from './scripts/rollup-wasm-plugin';
import pkg from './package.json';

function generateConfig() {
  const config = {
    input: './src/ts/index.ts',
    inlineDynamicImports: true,
    external: ['@solana/wasm', '*.wasm'],
    plugins: [
      rust({
        verbose: true,
        debug: true,
        cargoArgs: [],
        wasmName: 'solana',
      }),
      typescript(),
    ],
  };

  config.output = [
    {file: pkg.main, format: 'cjs'},
    {file: pkg.module, format: 'es'},
  ];

  return config;
}

export default [generateConfig()];
