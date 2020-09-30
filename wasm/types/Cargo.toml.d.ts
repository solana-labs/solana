// TODO: incorporate types from wasm-pack right now it doesn't work because typescript compiler runs before wasm-pack is complete
// export type Exports;// = typeof import('./../target/wasm-pack/solana-wasm/index');
declare const init: () => Promise<unknown>;
export default init;
