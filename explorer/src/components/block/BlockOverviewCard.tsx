import React from "react";
import { TableCardBody } from "components/common/TableCardBody";
import { useBlock, useFetchBlock, FetchStatus } from "providers/block";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { Slot } from "components/common/Slot";
import { ClusterStatus, useCluster } from "providers/cluster";
import { BlockHistoryCard } from "./BlockHistoryCard";
import { BlockRewardsCard } from "./BlockRewardsCard";

export function BlockOverviewCard({ slot }: { slot: number }) {
  const confirmedBlock = useBlock(slot);
  const fetchBlock = useFetchBlock();
  const { status } = useCluster();
  const refresh = () => fetchBlock(slot);

  // Fetch block on load
  React.useEffect(() => {
    if (!confirmedBlock && status === ClusterStatus.Connected) refresh();
  }, [slot, status]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!confirmedBlock || confirmedBlock.status === FetchStatus.Fetching) {
    return <LoadingCard message="Loading block" />;
  } else if (
    confirmedBlock.data === undefined ||
    confirmedBlock.status === FetchStatus.FetchFailed
  ) {
    return <ErrorCard retry={refresh} text="Failed to fetch block" />;
  } else if (confirmedBlock.data.block === undefined) {
    return <ErrorCard retry={refresh} text={`Block ${slot} was not found`} />;
  }

  const block = confirmedBlock.data.block;
  return (
    <>
      <div className="card">
        <div className="card-header">
          <h3 className="card-header-title mb-0 d-flex align-items-center">
            Overview
          </h3>
        </div>
        <TableCardBody>
          <tr>
            <td className="w-100">Slot</td>
            <td className="text-lg-right text-monospace">
              <Slot slot={slot} />
            </td>
          </tr>
          <tr>
            <td className="w-100">Parent Slot</td>
            <td className="text-lg-right text-monospace">
              <Slot slot={block.parentSlot} link />
            </td>
          </tr>
          <tr>
            <td className="w-100">Blockhash</td>
            <td className="text-lg-right text-monospace">
              <span>{block.blockhash}</span>
            </td>
          </tr>
          <tr>
            <td className="w-100">Previous Blockhash</td>
            <td className="text-lg-right text-monospace">
              <span>{block.previousBlockhash}</span>
            </td>
          </tr>
        </TableCardBody>
      </div>

      <BlockRewardsCard block={block} />
      <BlockHistoryCard block={block} />
    </>
  );
}
