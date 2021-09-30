import React from "react";
import { TableCardBody } from "components/common/TableCardBody";
import { useBlock, useFetchBlock, FetchStatus } from "providers/block";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { Slot } from "components/common/Slot";
import { ClusterStatus, useCluster } from "providers/cluster";
import { BlockHistoryCard } from "./BlockHistoryCard";
import { BlockRewardsCard } from "./BlockRewardsCard";
import { BlockResponse } from "@solana/web3.js";
import { NavLink } from "react-router-dom";
import { clusterPath } from "utils/url";
import { BlockProgramsCard } from "./BlockProgramsCard";
import { BlockAccountsCard } from "./BlockAccountsCard";
import { displayTimestamp, displayTimestampUtc } from "utils/date";

export function BlockOverviewCard({
  slot,
  tab,
}: {
  slot: number;
  tab?: string;
}) {
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
  const committedTxs = block.transactions.filter((tx) => tx.meta?.err === null);

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
            <td className="w-100">Blockhash</td>
            <td className="text-lg-right text-monospace">
              <span>{block.blockhash}</span>
            </td>
          </tr>
          {block.blockTime && (
            <>
              <tr>
                <td>Timestamp (Local)</td>
                <td className="text-lg-right">
                  <span className="text-monospace">
                    {displayTimestamp(block.blockTime * 1000, true)}
                  </span>
                </td>
              </tr>
              <tr>
                <td>Timestamp (UTC)</td>
                <td className="text-lg-right">
                  <span className="text-monospace">
                    {displayTimestampUtc(block.blockTime * 1000, true)}
                  </span>
                </td>
              </tr>
            </>
          )}
          <tr>
            <td className="w-100">Parent Slot</td>
            <td className="text-lg-right text-monospace">
              <Slot slot={block.parentSlot} link />
            </td>
          </tr>
          <tr>
            <td className="w-100">Parent Blockhash</td>
            <td className="text-lg-right text-monospace">
              <span>{block.previousBlockhash}</span>
            </td>
          </tr>
          {confirmedBlock.data.child && (
            <tr>
              <td className="w-100">Child Slot</td>
              <td className="text-lg-right text-monospace">
                <Slot slot={confirmedBlock.data.child} link />
              </td>
            </tr>
          )}
          <tr>
            <td className="w-100">Processed Transactions</td>
            <td className="text-lg-right text-monospace">
              <span>{block.transactions.length}</span>
            </td>
          </tr>
          <tr>
            <td className="w-100">Successful Transactions</td>
            <td className="text-lg-right text-monospace">
              <span>{committedTxs.length}</span>
            </td>
          </tr>
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
  return (
    <>
      <div className="container">
        <div className="header">
          <div className="header-body pt-0">
            <ul className="nav nav-tabs nav-overflow header-tabs">
              {TABS.map(({ title, slug, path }) => (
                <li key={slug} className="nav-item">
                  <NavLink
                    className="nav-link"
                    to={clusterPath(`/block/${slot}${path}`)}
                    exact
                  >
                    {title}
                  </NavLink>
                </li>
              ))}
            </ul>
          </div>
        </div>
      </div>
      {tab === undefined && <BlockHistoryCard block={block} />}
      {tab === "rewards" && <BlockRewardsCard block={block} />}
      {tab === "accounts" && <BlockAccountsCard block={block} />}
      {tab === "programs" && <BlockProgramsCard block={block} />}
    </>
  );
}
