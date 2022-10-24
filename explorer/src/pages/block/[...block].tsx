import React from "react";
import { useRouter } from "next/router";

import { ErrorCard } from "components/common/ErrorCard";
import { BlockOverviewCard } from "components/block/BlockOverviewCard";

// IE11 doesn't support Number.MAX_SAFE_INTEGER
const MAX_SAFE_INTEGER = 9007199254740991;

export function BlockDetailsPage() {
  const router = useRouter();
  const { block: queryParams } = router.query;
  let block = "";
  let tab: string | undefined;

  if (queryParams) {
    const [receivedBlock, receivedTab] = queryParams;
    block = receivedBlock;
    tab = receivedTab;
  }

  const blockSlot = Number(block);
  let output = <ErrorCard text={`Block ${block} is not valid`} />;

  if (
    !isNaN(blockSlot) &&
    blockSlot < MAX_SAFE_INTEGER &&
    blockSlot % 1 === 0
  ) {
    output = <BlockOverviewCard slot={blockSlot} tab={tab} />;
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

export default BlockDetailsPage;
