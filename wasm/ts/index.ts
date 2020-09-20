let exports: any;

export const ensureReady = () => {
    if(exports === undefined) {
        throw new Error(`@solana SDK not ready. You need to await waitReady function before making API calls`);
    }
}

export const waitReady = async () => {
    if(!exports)  {
        // @ts-ignore
        let wasm = await import('./../Cargo.toml');
        exports = await wasm.init();
    }
    
    return exports
}

export const getWASM = () => {
    ensureReady();
    return exports;
}