import React from "react";
import { useRouter } from "next/router";
import { TableCardBody } from "src/components/common/TableCardBody";
import { useBlock, useFetchBlock, FetchStatus } from "src/providers/block";
import { ErrorCard } from "src/components/common/ErrorCard";
import { LoadingCard } from "src/components/common/LoadingCard";
import { Slot } from "src/components/common/Slot";
import { ClusterStatus, useCluster } from "src/providers/cluster";
import { BlockHistoryCard } from "./BlockHistoryCard";
import { BlockRewardsCard } from "./BlockRewardsCard";
import { BlockResponse } from "@solana/web3.js";
import { NavLink } from "src/components/NavLink";
import { clusterPath } from "src/utils/url";
import { BlockProgramsCard } from "./BlockProgramsCard";
import { BlockAccountsCard } from "./BlockAccountsCard";
import { displayTimestamp, displayTimestampUtc } from "src/utils/date";
import { Epoch } from "src/components/common/Epoch";
import { Address } from "src/components/common/Address";

export function BlockOverviewCard({
  slot,
  tab,
}: {
  slot: number;
  tab?: string;
}) {
  const confirmedBlock = useBlock(slot);
  const fetchBlock = useFetchBlock();
  const { clusterInfo, status } = useCluster();
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

  const { block, blockLeader, childSlot, childLeader, parentLeader } =
    confirmedBlock.data;
  const showSuccessfulCount = block.transactions.every(
    (tx) => tx.meta !== null
  );
  const successfulTxs = block.transactions.filter(
    (tx) => tx.meta?.err === null
  );
  const epoch = clusterInfo?.epochSchedule.getEpoch(slot);

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
            <td className="w-100">Blockhash</td>
            <td className="text-lg-end font-monospace">
              <span>{block.blockhash}</span>
            </td>
          </tr>
          <tr>
            <td className="w-100">Slot</td>
            <td className="text-lg-end font-monospace">
              <Slot slot={slot} />
            </td>
          </tr>
          {blockLeader !== undefined && (
            <tr>
              <td className="w-100">Slot Leader</td>
              <td className="text-lg-end">
                <Address pubkey={blockLeader} alignRight link />
              </td>
            </tr>
          )}
          {block.blockTime ? (
            <>
              <tr>
                <td>Timestamp (Local)</td>
                <td className="text-lg-end">
                  <span className="font-monospace">
                    {displayTimestamp(block.blockTime * 1000, true)}
                  </span>
                </td>
              </tr>
              <tr>
                <td>Timestamp (UTC)</td>
                <td className="text-lg-end">
                  <span className="font-monospace">
                    {displayTimestampUtc(block.blockTime * 1000, true)}
                  </span>
                </td>
              </tr>
            </>
          ) : (
            <tr>
              <td className="w-100">Timestamp</td>
              <td className="text-lg-end">Unavailable</td>
            </tr>
          )}
          {epoch !== undefined && (
            <tr>
              <td className="w-100">Epoch</td>
              <td className="text-lg-end font-monospace">
                <Epoch epoch={epoch} link />
              </td>
            </tr>
          )}
          <tr>
            <td className="w-100">Parent Blockhash</td>
            <td className="text-lg-end font-monospace">
              <span>{block.previousBlockhash}</span>
            </td>
          </tr>
          <tr>
            <td className="w-100">Parent Slot</td>
            <td className="text-lg-end font-monospace">
              <Slot slot={block.parentSlot} link />
            </td>
          </tr>
          {parentLeader !== undefined && (
            <tr>
              <td className="w-100">Parent Slot Leader</td>
              <td className="text-lg-end">
                <Address pubkey={parentLeader} alignRight link />
              </td>
            </tr>
          )}
          {childSlot !== undefined && (
            <tr>
              <td className="w-100">Child Slot</td>
              <td className="text-lg-end font-monospace">
                <Slot slot={childSlot} link />
              </td>
            </tr>
          )}
          {childLeader !== undefined && (
            <tr>
              <td className="w-100">Child Slot Leader</td>
              <td className="text-lg-end">
                <Address pubkey={childLeader} alignRight link />
              </td>
            </tr>
          )}
          <tr>
            <td className="w-100">Processed Transactions</td>
            <td className="text-lg-end font-monospace">
              <span>{block.transactions.length}</span>
            </td>
          </tr>
          {showSuccessfulCount && (
            <tr>
              <td className="w-100">Successful Transactions</td>
              <td className="text-lg-end font-monospace">
                <span>{successfulTxs.length}</span>
              </td>
            </tr>
          )}
        </TableCardBody>
      </div>

      <MoreSection block={block} slot={slot} tab={tab} />
    </>
  );
}

const TABS: Tab[] = [
  {
    slug: "history",
    title: "Transactions",
    path: "",
  },
  {
    slug: "rewards",
    title: "Rewards",
    path: "/rewards",
  },
  {
    slug: "programs",
    title: "Programs",
    path: "/programs",
  },
  {
    slug: "accounts",
    title: "Accounts",
    path: "/accounts",
  },
];

type MoreTabs = "history" | "rewards" | "programs" | "accounts";

type Tab = {
  slug: MoreTabs;
  title: string;
  path: string;
};

function MoreSection({
  slot,
  block,
  tab,
}: {
  slot: number;
  block: BlockResponse;
  tab?: string;
}) {
  const router = useRouter();

  return (
    <>
      <div className="container">
        <div className="header">
          <div className="header-body pt-0">
            <ul className="nav nav-tabs nav-overflow header-tabs">
              {TABS.map(({ title, slug, path }) => (
                <li key={slug} className="nav-item">
                  <NavLink
                    activeClassName="active"
                    href={clusterPath(`/block/${slot}${path}`, router.asPath)}
                  >
                    <span className="nav-link">
                      {title}
                    </span>
                  </NavLink>
                </li>
              ))}
            </ul>
          </div>
        </div>
      </div>
      {tab === undefined && <BlockHistoryCard block={block} />}
      {tab === "rewards" && <BlockRewardsCard block={block} />}
      {tab === "accounts" && (
        <BlockAccountsCard block={block} blockSlot={slot} />
      )}
      {tab === "programs" && <BlockProgramsCard block={block} />}
    </>
  );
}
