let cargo: any; // TODO: figure out a way to link to types

export const ensureReady = () => {
  if (cargo === undefined) {
    throw new Error(
      `@solana/wasm package is not ready. You need to await waitReady() before making API calls`,
    );
  }
};

export const waitReady = async () => {
  if (!cargo) {
    // @ts-ignore
    const wasm = await import('./../../Cargo.toml');
    cargo = await wasm.init();
  }

  return cargo;
};

export const getWASM = () => {
  ensureReady();
  return cargo;
};
