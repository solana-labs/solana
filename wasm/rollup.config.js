import typescript from "rollup-plugin-typescript2";
import rust from "./scripts/rollup-wasm-plugin";
import pkg from "./package.json";

function generateConfig(configType) {
  const config = {
    input: "./ts/index.ts",
    inlineDynamicImports: true,
    external: ["@solana/wasm", "*.wasm"],
    plugins: [
      rust({
        verbose: true,
        debug: true,
        cargoArgs: [],
        wasmName: "solana",
      }),
      typescript(),
    ],
  };

  switch (configType) {
    case "browser":
      config.output = [
        {
          file: "lib/index.iife.js",
          format: "iife",
          name: "solana-wasm-web",
          sourcemap: true,
        },
      ];
      config.plugins.push(builtins());
      config.plugins.push(globals());
      config.plugins.push(
        nodeResolve({
          browser: true,
        })
      );

      if (env === "production") {
        config.plugins.push(
          terser({
            mangle: false,
            compress: false,
          })
        );
      }

      break;
    case "node":
      config.output = [
        { file: pkg.main, format: "cjs" },
        { file: pkg.module, format: "es" },
      ];

      break;
    default:
      throw new Error(`Unknown configType: ${configType}`);
  }

  return config;
}

export default [generateConfig("node")];
