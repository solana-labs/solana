import React from "react";

import { ErrorCard } from "components/common/ErrorCard";
import { BlockOverviewCard } from "components/block/BlockOverviewCard";

type Props = { slot: string };

export function BlockDetailsPage({ slot }: Props) {
  let output = <ErrorCard text={`Block ${slot} is not valid`} />;

  if (!isNaN(Number(slot))) {
    output = <BlockOverviewCard slot={Number(slot)} />;
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
