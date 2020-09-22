const path = require("path");
const { override, addExternalBabelPlugins } = require("customize-cra");
const WASM = require('@solana/wasm/webpack');

module.exports = override(
  ...addExternalBabelPlugins(WASM.BabelPlugin),
  (config) =>{
    const wasmExtensionRegExp = /\.wasm$/;

    config.resolve.extensions.push(".wasm");

    config.module.rules.forEach((rule) => {
      (rule.oneOf || []).forEach((oneOf) => {
        if (oneOf.loader && oneOf.loader.indexOf("file-loader") >= 0) {
          // Make file-loader ignore WASM files
          oneOf.exclude.push(wasmExtensionRegExp);
        }
      });
    });

    // Add a dedicated loader for WASM
    config.module.rules.push({
      test: wasmExtensionRegExp,
      include: path.resolve(__dirname, "src"),
      use: [{ loader: require.resolve("wasm-loader"), options: {} }],
    });

    config.plugins.push(new WASM.WebpackPlugin());

    return config;
  },
);
