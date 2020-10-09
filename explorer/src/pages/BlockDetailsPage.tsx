import React from "react";

import { BlockHistoryCard } from "components/account/BlockHistoryCard";
import { ErrorCard } from "components/common/ErrorCard";

export const SolarweaveDatabase = "solarweave-cache-devnet-testrun4-index";

type Props = { slot: string; blockhash: string };

export function BlockDetailsPage({ slot, blockhash }: Props) {
  let output = <ErrorCard text={`Block is not valid`} />;

  if (slot || blockhash) {
    output = <BlockHistoryCard slot={slot} blockhash={blockhash} />;
  }

  return (
    <div className="container mt-n3">
      <div className="header">
        <div className="header-body">
          <h6 className="header-pretitle">Details</h6>
          <h2 className="header-title">Block</h2>
        </div>
      </div>
      {output}
    </div>
  );
}
