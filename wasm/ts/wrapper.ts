let exports: any; // TODO: figure out a way to link to types

export const ensureReady = () => {
    if(exports === undefined) {
        throw new Error(`@solana/wasm package is not ready. You need to await waitReady() before making API calls`);
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