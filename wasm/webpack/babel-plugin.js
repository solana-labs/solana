const pathInternal = require("path");
const eventEmitter = require("./event-listener");
const t = require("@babel/types");

module.exports = function () {
  return {
    visitor: {
      CallExpression(path, state) {
        if (path.node.callee.name === "fetch") {
          console.log("WASM BABEL plugin");
          const argument = JSON.parse(JSON.stringify(path.node.arguments[0]));
          if (argument && argument.value && argument.value.endsWith(".wasm")) {
            // reset value for wasm can work correctly
            if (argument.value.indexOf(eventEmitter.wasmOutDir) >= 0) {
              return;
            }

            eventEmitter.emit("wasm", {
              wasmRefPath: argument.value,
              wasmDir: pathInternal.parse(state.file.opts.filename).dir,
            });

            const updatedPath = pathInternal.join(
              "/",
              eventEmitter.wasmOutDir,
              argument.value
            );
            console.log(updatedPath);
            path.node.arguments[0] = t.stringLiteral(updatedPath); //{"type":"StringLiteral","value":updatedPath};
          }
        }
      },
    },
  };
};
