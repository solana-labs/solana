import React from "react";

import { ErrorCard } from "components/common/ErrorCard";
import { BlockOverviewCard } from "components/block/BlockOverviewCard";

type Props = { slot: string };

export function BlockDetailsPage({ slot }: Props) {
  const slotNumber = Number(slot);
  let output = <ErrorCard text={`Block ${slot} is not valid`} />;

  if (!isNaN(slotNumber) && slotNumber < Number.MAX_SAFE_INTEGER) {
    output = <BlockOverviewCard slot={slotNumber} />;
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
