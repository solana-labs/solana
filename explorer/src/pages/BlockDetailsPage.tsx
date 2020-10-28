import React from "react";

import { BlockHistoryCard } from "components/account/BlockHistoryCard";
import { ErrorCard } from "components/common/ErrorCard";

type Props = { slot: string };

export function BlockDetailsPage({ slot }: Props) {
  let output = <ErrorCard text={`Block ${slot} is not valid`} />;

  if (!isNaN(Number(slot))) {
    output = <BlockHistoryCard slot={Number(slot)} />;
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
