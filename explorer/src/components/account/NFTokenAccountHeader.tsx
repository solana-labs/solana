import React, { Suspense } from "react";
import { Account } from "../../providers/accounts";
import {
  parseNFTokenCollectionAccount,
  parseNFTokenNFTAccount,
} from "../../utils/nftoken-parsers.ts/isNFTokenAccount";
import { NftokenTypes } from "../../utils/nftoken-types";
import { InfoTooltip } from "../common/InfoTooltip";
import { CachedImageContent } from "../common/NFTArt";
import { useNftokenMetadata } from "./nftoken-hooks";

export function NFTokenAccountHeader({ account }: { account: Account }) {
  const nft = parseNFTokenNFTAccount(account);

  if (nft) {
    return (
      <Suspense fallback={<div>Loading...</div>}>
        <NFTokenNFTHeader nft={nft} />
      </Suspense>
    );
  }

  const collection = parseNFTokenCollectionAccount(account);
  if (collection) {
    return (
      <Suspense fallback={<div>Loading...</div>}>
        <NFTokenCollectionHeader collection={collection} />
      </Suspense>
    );
  }

  return (
    <>
      <h6 className="header-pretitle">Details</h6>
      <h2 className="header-title">Account</h2>
    </>
  );
}

export function NFTokenNFTHeader({ nft }: { nft: NftokenTypes.NftAccount }) {
  const { data: metadata } = useNftokenMetadata(nft.metadata_url);

  return (
    <div className="row">
      <div className="col-auto ms-2 d-flex align-items-center">
        <CachedImageContent uri={metadata?.image} />
      </div>

      <div className="col mb-3 ms-0.5 mt-3">
        {<h6 className="header-pretitle ms-1">NFToken NFT</h6>}
        <div className="d-flex align-items-center">
          <h2 className="header-title ms-1 align-items-center no-overflow-with-ellipsis">
            {metadata ? metadata.name || "No NFT name was found" : "Loading..."}
          </h2>
        </div>

        <div>
          <div className={"d-inline-flex align-items-center mt-2"}>
            <span className="badge badge-pill bg-dark">{`${
              nft.authority_can_update ? "Mutable" : "Immutable"
            }`}</span>

            <InfoTooltip
              bottom
              text={
                nft.authority_can_update
                  ? "The authority of this NFT can update the Metadata."
                  : "The Metadata cannot be updated by anyone."
              }
            />
          </div>
        </div>
      </div>
    </div>
  );
}

export function NFTokenCollectionHeader({
  collection,
}: {
  collection: NftokenTypes.CollectionAccount;
}) {
  const { data: metadata } = useNftokenMetadata(collection.metadata_url);

  return (
    <div className="row">
      <div className="col-auto ms-2 d-flex align-items-center">
        <CachedImageContent uri={metadata?.image} />
      </div>

      <div className="col mb-3 ms-0.5 mt-3">
        {<h6 className="header-pretitle ms-1">NFToken Collection</h6>}
        <div className="d-flex align-items-center">
          <h2 className="header-title ms-1 align-items-center no-overflow-with-ellipsis">
            {metadata
              ? metadata.name || "No collection name was found"
              : "Loading..."}
          </h2>
        </div>

        <div>
          <div className={"d-inline-flex align-items-center mt-2"}>
            <span className="badge badge-pill bg-dark">{`${
              collection.authority_can_update ? "Mutable" : "Immutable"
            }`}</span>

            <InfoTooltip
              bottom
              text={
                collection.authority_can_update
                  ? "The authority of this Collection can update the Metadata and add NFTs."
                  : "The Metadata cannot be updated by anyone."
              }
            />
          </div>
        </div>
      </div>
    </div>
  );
}
