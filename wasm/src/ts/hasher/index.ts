import {getWASM, ensureReady} from './../wrapper';

export function sha256(data: Uint8Array): Uint8Array {
  ensureReady();
  return getWASM().sha256(data);
}
