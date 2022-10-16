import React from "react";
import {
  NFTData,
  useFetchAccountInfo,
  useMintAccountInfo,
} from "providers/accounts";
import { programs } from "@metaplex/js";
import { ArtContent } from "components/common/NFTArt";
import { InfoTooltip } from "components/common/InfoTooltip";
import { clusterPath } from "utils/url";
import { Link } from "react-router-dom";
import { EditionInfo } from "providers/accounts/utils/getEditionInfo";
import { PublicKey } from "@solana/web3.js";

export function MetaplexNFTHeader({
  nftData,
  address,
}: {
  nftData: NFTData;
  address: string;
}) {
  const collectionAddress = nftData.metadata.collection?.key;
  const collectionMintInfo = useMintAccountInfo(collectionAddress);
  const fetchAccountInfo = useFetchAccountInfo();

  React.useEffect(() => {
    if (collectionAddress && !collectionMintInfo) {
      fetchAccountInfo(new PublicKey(collectionAddress), "parsed");
    }
  }, [fetchAccountInfo, collectionAddress]); // eslint-disable-line react-hooks/exhaustive-deps

  const metadata = nftData.metadata;
  const data = nftData.json;
  const isVerifiedCollection =
    metadata.collection != null &&
    metadata.collection?.verified &&
    collectionMintInfo !== undefined;
  return (
    <div className="row">
      <div className="col-auto ms-2 d-flex align-items-center">
        <ArtContent metadata={metadata} pubkey={address} data={data} />
      </div>
      <div className="col mb-3 ms-0.5 mt-3">
        {<h6 className="header-pretitle ms-1">Metaplex NFT</h6>}
        <div className="d-flex align-items-center">
          <h2 className="header-title ms-1 align-items-center no-overflow-with-ellipsis">
            {metadata.data.name !== ""
              ? metadata.data.name
              : "No NFT name was found"}
          </h2>
          {getEditionPill(nftData.editionInfo)}
          {isVerifiedCollection ? getVerifiedCollectionPill() : null}
        </div>
        <h4 className="header-pretitle ms-1 mt-1 no-overflow-with-ellipsis">
          {metadata.data.symbol !== ""
            ? metadata.data.symbol
            : "No Symbol was found"}
        </h4>
        <div className="mb-2 mt-2">
          {getSaleTypePill(metadata.primarySaleHappened)}
        </div>
        <div className="mb-3 mt-2">{getIsMutablePill(metadata.isMutable)}</div>
        <div className="btn-group">
          <button
            className="btn btn-dark btn-sm dropdown-toggle creators-dropdown-button-width"
            type="button"
            data-bs-toggle="dropdown"
            aria-haspopup="true"
            aria-expanded="false"
          >
            Creators
          </button>
          <div className="dropdown-menu mt-2">
            {getCreatorDropdownItems(metadata.data.creators)}
          </div>
        </div>
      </div>
    </div>
  );
}

type Creator = programs.metadata.Creator;
function getCreatorDropdownItems(creators: Creator[] | null) {
  const CreatorHeader = () => {
    const creatorTooltip =
      "Verified creators signed the metadata associated with this NFT when it was created.";

    const shareTooltip =
      "The percentage of the proceeds a creator receives when this NFT is sold.";

    return (
      <div
        className={
          "d-flex align-items-center dropdown-header creator-dropdown-entry"
        }
      >
        <div className="d-flex font-monospace creator-dropdown-header">
          <span>Creator Address</span>
          <InfoTooltip bottom text={creatorTooltip} />
        </div>
        <div className="d-flex font-monospace">
          <span className="font-monospace">Royalty</span>
          <InfoTooltip bottom text={shareTooltip} />
        </div>
      </div>
    );
  };

  const getVerifiedIcon = (isVerified: boolean) => {
    const className = isVerified ? "fe fe-check" : "fe fe-alert-octagon";
    return <i className={`ms-3 ${className}`}></i>;
  };

  const CreatorEntry = (creator: Creator) => {
    return (
      <div
        className={
          "d-flex align-items-center font-monospace creator-dropdown-entry ms-3 me-3"
        }
      >
        {getVerifiedIcon(creator.verified)}
        <Link
          className="dropdown-item font-monospace creator-dropdown-entry-address"
          to={clusterPath(`/address/${creator.address}`)}
        >
          {creator.address}
        </Link>
        <div className="me-3"> {`${creator.share}%`}</div>
      </div>
    );
  };

  if (creators && creators.length > 0) {
    let listOfCreators: JSX.Element[] = [];

    listOfCreators.push(<CreatorHeader key={"header"} />);
    creators.forEach((creator) => {
      listOfCreators.push(<CreatorEntry key={creator.address} {...creator} />);
    });

    return listOfCreators;
  }

  return (
    <div className={"dropdown-item font-monospace"}>
      <div className="me-3">No creators are associated with this NFT.</div>
    </div>
  );
}

function getEditionPill(editionInfo: EditionInfo) {
  const masterEdition = editionInfo.masterEdition;
  const edition = editionInfo.edition;

  return (
    <div className={"d-inline-flex ms-2"}>
      <span className="badge badge-pill bg-dark">{`${
        edition && masterEdition
          ? `Edition ${edition.edition.toNumber()} / ${masterEdition.supply.toNumber()}`
          : masterEdition
          ? "Master Edition"
          : "No Master Edition Information"
      }`}</span>
    </div>
  );
}

function getSaleTypePill(hasPrimarySaleHappened: boolean) {
  const primaryMarketTooltip =
    "Creator(s) split 100% of the proceeds when this NFT is sold.";

  const secondaryMarketTooltip =
    "Creator(s) split the Seller Fee when this NFT is sold. The owner receives the remaining proceeds.";

  return (
    <div className={"d-inline-flex align-items-center"}>
      <span className="badge badge-pill bg-dark">{`${
        hasPrimarySaleHappened ? "Secondary Market" : "Primary Market"
      }`}</span>
      <InfoTooltip
        bottom
        text={
          hasPrimarySaleHappened ? secondaryMarketTooltip : primaryMarketTooltip
        }
      />
    </div>
  );
}

function getIsMutablePill(isMutable: boolean) {
  return (
    <span className="badge badge-pill bg-dark">{`${
      isMutable ? "Mutable" : "Immutable"
    }`}</span>
  );
}

function getVerifiedCollectionPill() {
  const onchainVerifiedToolTip =
    "This NFT has been verified as a member of an on-chain collection. This tag guarantees authenticity.";
  return (
    <div className={"d-inline-flex align-items-center ms-2"}>
      <span className="badge badge-pill bg-dark">{"Verified Collection"}</span>
      <InfoTooltip bottom text={onchainVerifiedToolTip} />
    </div>
  );
}
