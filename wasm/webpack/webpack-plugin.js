const path = require("path");
const eventEmitter = require("./event-listener");

class WasmPlugin {
  apply(compiler) {
    // get webpack config
    const webpackOptions = compiler.options;
    // get js out path
    const jsOutFileName = webpackOptions.output.filename;
    const jsOutDirPath = jsOutFileName.substring(
      0,
      jsOutFileName.lastIndexOf("/")
    );
    // cache
    eventEmitter.emit("out-dir", jsOutDirPath);
    compiler.plugin("emit", function (compilation, callback) {
      for (const i in eventEmitter.data) {
        // wasm resource path
        const filePath = path.join(eventEmitter.data[i], i);
        const content = compiler.inputFileSystem._readFileSync(filePath);
        const stat = compiler.inputFileSystem._statSync(filePath);
        // generate filename with js out path
        const wasmName = path.join(jsOutDirPath, i);
        // output
        compilation.assets[wasmName] = {
          size() {
            return stat.size;
          },
          source() {
            return content;
          },
        };

        console.log(compilation);
      }
      callback();
    });
  }
}

module.exports = WasmPlugin;
