import React from "react";
import { Link } from "react-router-dom";
import { clusterPath } from "../../../utils/url";
import { useCollectionNfts } from "./nftoken-hooks";
import { NftokenImage } from "./NFTokenAccountSection";

export function NFTokenCollectionNFTGrid({
  collection,
}: {
  collection: string;
}) {
  const { data: nfts, mutate } = useCollectionNfts({
    collectionAddress: collection,
  });

  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">NFTs</h3>

        <button className="btn btn-white btn-sm" onClick={() => mutate()}>
          <span className="fe fe-refresh-cw me-2"></span>
          Refresh
        </button>
      </div>

      <div className="py-4">
        {nfts.length === 0 && <div className={"px-4"}>No NFTs Found</div>}

        {nfts.length > 0 && (
          <div
            style={{
              display: "grid",
              /* Creates as many columns as possible that are at least 10rem wide. */
              gridTemplateColumns: "repeat(auto-fill, minmax(10rem, 1fr))",
              gridGap: "1.5rem",
            }}
          >
            {nfts.map((nft) => (
              <div
                key={nft.address}
                style={{
                  display: "flex",
                  flexDirection: "column",
                  justifyContent: "center",
                  alignItems: "center",
                  gap: "1rem",
                }}
              >
                <NftokenImage url={nft.image} size={80} />

                <div>
                  <Link to={clusterPath(`/address/${nft.address}`)}>
                    <div>{nft.name ?? "No Name"}</div>
                  </Link>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
