import {
  NFTOKEN_ADDRESS,
  NFTOKEN_COLLECTION_ACCOUNT,
  NFTOKEN_NFT_ACCOUNT,
  NftokenTypes,
} from "@glow-xyz/nftoken-js";
import { Account } from "../index";

export function isNFTokenAccount(account?: Account): boolean {
  return Boolean(
    account?.details?.owner.toBase58() === NFTOKEN_ADDRESS &&
      account?.details.rawData
  );
}

export const parseNFTokenNFTAccount = (
  account: Account | undefined
): NftokenTypes.Nft | null => {
  if (!isNFTokenAccount(account)) {
    return null;
  }

  const parsed = NFTOKEN_NFT_ACCOUNT.parse({
    buffer: account!.details!.rawData!,
  });

  if (!parsed) {
    return null;
  }

  return { address: account!.pubkey.toBase58(), ...parsed };
};

export const parseNFTokenCollectionAccount = (
  account: Account | undefined
): NftokenTypes.Collection | null => {
  if (!isNFTokenAccount(account)) {
    return null;
  }

  const parsed = NFTOKEN_COLLECTION_ACCOUNT.parse({
    buffer: account!.details!.rawData!,
  });

  if (!parsed) {
    return null;
  }

  return {
    address: account!.pubkey.toBase58(),
    // This satisfies the types but should be removed later on
    network: "mainnet",
    ...parsed,
  };
};
