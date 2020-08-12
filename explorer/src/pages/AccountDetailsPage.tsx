import React from "react";
import { PublicKey } from "@solana/web3.js";
import { FetchStatus } from "providers/cache";
import {
  useFetchAccountInfo,
  useAccountInfo,
  Account,
} from "providers/accounts";
import { StakeAccountSection } from "components/account/StakeAccountSection";
import { TokenAccountSection } from "components/account/TokenAccountSection";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { useCluster, ClusterStatus } from "providers/cluster";
import { NavLink, Redirect, useLocation } from "react-router-dom";
import { clusterPath } from "utils/url";
import { UnknownAccountCard } from "components/account/UnknownAccountCard";
import { OwnedTokensCard } from "components/account/OwnedTokensCard";
import { TransactionHistoryCard } from "components/account/TransactionHistoryCard";
import { TokenHistoryCard } from "components/account/TokenHistoryCard";
import { TokenLargestAccountsCard } from "components/account/TokenLargestAccountsCard";

type Props = { address: string; tab?: string };
export function AccountDetailsPage({ address, tab }: Props) {
  let pubkey: PublicKey | undefined;

  try {
    pubkey = new PublicKey(address);
  } catch (err) {}

  return (
    <div className="container mt-n3">
      <div className="header">
        <div className="header-body">
          <h6 className="header-pretitle">Details</h6>
          <h4 className="header-title">Account</h4>
        </div>
      </div>
      {!pubkey ? (
        <ErrorCard text={`Address "${address}" is not valid`} />
      ) : (
        <DetailsSections pubkey={pubkey} tab={tab} />
      )}
    </div>
  );
}

function DetailsSections({ pubkey, tab }: { pubkey: PublicKey; tab?: string }) {
  const fetchAccount = useFetchAccountInfo();
  const address = pubkey.toBase58();
  const info = useAccountInfo(address);
  const { status } = useCluster();
  const location = useLocation();

  // Fetch account on load
  React.useEffect(() => {
    if (!info && status === ClusterStatus.Connected) fetchAccount(pubkey);
  }, [address, status]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!info || info.status === FetchStatus.Fetching) {
    return <LoadingCard />;
  } else if (
    info.status === FetchStatus.FetchFailed ||
    info.data?.lamports === undefined
  ) {
    return <ErrorCard retry={() => fetchAccount(pubkey)} text="Fetch Failed" />;
  }

  const account = info.data;
  const data = account?.details?.data;

  let tabs: Tab[] = [
    {
      slug: "history",
      title: "History",
      path: "",
    },
  ];

  if (data && data?.name === "spl-token") {
    if (data.parsed.type === "mint") {
      tabs.push({
        slug: "holders",
        title: "Holders",
        path: "/holders",
      });
    }
  } else {
    tabs.push({
      slug: "tokens",
      title: "Tokens",
      path: "/tokens",
    });
  }

  let moreTab: MoreTabs = "history";
  if (tab && tabs.filter(({ slug }) => slug === tab).length === 0) {
    return <Redirect to={{ ...location, pathname: `/address/${address}` }} />;
  } else if (tab) {
    moreTab = tab as MoreTabs;
  }

  return (
    <>
      {<InfoSection account={account} />}
      {<MoreSection account={account} tab={moreTab} tabs={tabs} />}
    </>
  );
}

function InfoSection({ account }: { account: Account }) {
  const data = account?.details?.data;
  if (data && data.name === "stake") {
    let stakeAccountType, stakeAccount;
    if ("accountType" in data.parsed) {
      stakeAccount = data.parsed;
      stakeAccountType = data.parsed.accountType as any;
    } else {
      stakeAccount = data.parsed.info;
      stakeAccountType = data.parsed.type;
    }

    return (
      <StakeAccountSection
        account={account}
        stakeAccount={stakeAccount}
        stakeAccountType={stakeAccountType}
      />
    );
  } else if (data && data.name === "spl-token") {
    return <TokenAccountSection account={account} tokenAccount={data.parsed} />;
  } else {
    return <UnknownAccountCard account={account} />;
  }
}

type Tab = {
  slug: MoreTabs;
  title: string;
  path: string;
};

type MoreTabs = "history" | "tokens" | "holders";
function MoreSection({
  account,
  tab,
  tabs,
}: {
  account: Account;
  tab: MoreTabs;
  tabs: Tab[];
}) {
  const pubkey = account.pubkey;
  const address = account.pubkey.toBase58();

  return (
    <>
      <div className="container">
        <div className="header">
          <div className="header-body pt-0">
            <ul className="nav nav-tabs nav-overflow header-tabs">
              {tabs.map(({ title, path }) => (
                <li className="nav-item">
                  <NavLink
                    className="nav-link"
                    to={clusterPath(`/address/${address}${path}`)}
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
      {tab === "tokens" && (
        <>
          <OwnedTokensCard pubkey={pubkey} />
          <TokenHistoryCard pubkey={pubkey} />
        </>
      )}
      {tab === "history" && <TransactionHistoryCard pubkey={pubkey} />}
      {tab === "holders" && <TokenLargestAccountsCard pubkey={pubkey} />}
    </>
  );
}
