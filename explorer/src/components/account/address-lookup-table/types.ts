import { PublicKey } from "@solana/web3.js";

const PROGRAM_ID: string = "AddressLookupTab1e1111111111111111111111111";

export function isAddressLookupTableAccount(
  accountOwner: PublicKey,
  accountData: Uint8Array
): boolean {
  if (accountOwner.toBase58() !== PROGRAM_ID) return false;
  if (!accountData || accountData.length === 0) return false;
  const LOOKUP_TABLE_ACCOUNT_TYPE = 1;
  return accountData[0] === LOOKUP_TABLE_ACCOUNT_TYPE;
}
