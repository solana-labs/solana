import { NFTOKEN_ADDRESS } from "../nftoken";
import { Account } from "../../providers/accounts";
import { NftokenTypes } from "../nftoken-types";

export function isNFTokenAccount(account?: Account): boolean {
  return Boolean(
    account?.details?.owner.toBase58() === NFTOKEN_ADDRESS &&
      account?.details.rawData
  );
}

export const parseNFTokenNFTAccount = (
  account: Account | undefined
): NftokenTypes.NftAccount | null => {
  if (!isNFTokenAccount(account)) {
    return null;
  }

  const parsed = NftokenTypes.nftAccountLayout.decode(
    account!.details!.rawData!
  );

  if (!parsed) {
    return null;
  }

  return {
    address: account!.pubkey.toBase58(),
    holder: parsed.holder,
    authority: parsed.authority,
    authority_can_update: Boolean(parsed.authority_can_update),

    collection: parsed.collection,
    delegate: parsed.delegate,

    metadata_url: parsed.metadata_url,
  };
};

export const parseNFTokenCollectionAccount = (
  account: Account | undefined
): NftokenTypes.CollectionAccount | null => {
  if (!isNFTokenAccount(account)) {
    return null;
  }

  const parsed = NftokenTypes.collectionAccountLayout.decode(
    account!.details!.rawData!
  );

  if (!parsed) {
    return null;
  }

  return {
    address: account!.pubkey.toBase58(),
    authority: parsed.authority,
    authority_can_update: Boolean(parsed.authority_can_update),
    metadata_url: parsed.metadata_url,
  };
};
