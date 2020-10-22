import React from "react";

import { BlockHistoryCard } from "components/account/BlockHistoryCard";
import { ErrorCard } from "components/common/ErrorCard";

type Props = { block: string };

export function BlockDetailsPage({ block }: Props) {
  let output = <ErrorCard text={`Block ${block} is not valid`} />;

  if (block) {
    output = <BlockHistoryCard block={block} />;
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
