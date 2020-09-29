const EventEmitter = require("events").EventEmitter;

class Events extends EventEmitter {
  constructor(prop) {
    super(prop);
    this.data = {};
    this.wasmOutDir = "";
  }
}
const events = new Events();
events.on("wasm", (data) => {
  if (!events.data[data.wasmRefPath]) {
    events.data[data.wasmRefPath] = data.wasmDir;
  }
});
events.on("out-dir", (data) => {
  if (!events.wasmOutDir) {
    events.wasmOutDir = data;
  }
});
module.exports = events;
