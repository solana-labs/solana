import { PublicKey } from "@solana/web3.js";
import { NFTOKEN_ADDRESS } from "../nftoken";
import { Account } from "../../providers/accounts";
import { NftokenTypes } from "../nftoken-types";

export function isNFTokenAccount(account: Account): boolean {
  return Boolean(
    account.details?.owner.toBase58() === NFTOKEN_ADDRESS &&
      account.details.rawData
  );
}

export const parseNFTokenNFTAccount = (
  account: Account
): NftokenTypes.NftAccount | null => {
  if (!isNFTokenAccount(account)) {
    return null;
  }

  try {
    const parsed = NftokenTypes.nftAccountLayout.decode(
      account!.details!.rawData!
    );

    if (!parsed) {
      return null;
    }

    if (
      Buffer.from(parsed!.discriminator).toString("base64") !== "IbRbNewPP2E="
    ) {
      return null;
    }

    return {
      address: account!.pubkey.toBase58(),
      holder: new PublicKey(parsed.holder).toBase58(),
      authority: new PublicKey(parsed.authority).toBase58(),
      authority_can_update: Boolean(parsed.authority_can_update),

      collection: new PublicKey(parsed.collection).toBase58(),
      delegate: new PublicKey(parsed.delegate).toBase58(),

      metadata_url: parsed.metadata_url?.replace(/\0/g, "") ?? null,
    };
  } catch (e) {
    console.error("Problem parsing NFToken NFT...", e);
    return null;
  }
};

export const parseNFTokenCollectionAccount = (
  account: Account
): NftokenTypes.CollectionAccount | null => {
  if (!isNFTokenAccount(account)) {
    return null;
  }

  try {
    const parsed = NftokenTypes.collectionAccountLayout.decode(
      account!.details!.rawData!
    );

    if (!parsed) {
      return null;
    }
    if (
      Buffer.from(parsed.discriminator).toString("base64") !== "RQLwA3YS2fI="
    ) {
      return null;
    }

    return {
      address: account!.pubkey.toBase58(),
      authority: parsed.authority,
      authority_can_update: Boolean(parsed.authority_can_update),
      metadata_url: parsed.metadata_url?.replace(/\0/g, "") ?? null,
    };
  } catch (e) {
    console.error("Problem parsing NFToken Collection...", e);
    return null;
  }
};
