import { getWASM, waitReady } from './../wrapper';

export async function sha256(data: Uint8Array): Promise<Uint8Array> {
  await waitReady();
  return getWASM().sha256(data);
}
