const path = require("path");
const { override, addExternalBabelPlugins } = require("customize-cra");
const WASM = require('@solana/wasm/webpack');

module.exports = override(
  ...addExternalBabelPlugins(WASM.BabelPlugin),
  (config) =>{
    config.module.rules.forEach((rule) => {
      (rule.oneOf || []).forEach((oneOf) => {
        if (oneOf.loader && oneOf.loader.indexOf("file-loader") >= 0) {
          // Make file-loader ignore WASM files
          oneOf.exclude.push(/\.wasm$/);
        }
      });
    });

    config.plugins.push(new WASM.WebpackPlugin());

    return config;
  },
);
