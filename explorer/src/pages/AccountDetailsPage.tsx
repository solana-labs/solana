import React from "react";
import { PublicKey } from "@solana/web3.js";
import {
  FetchStatus,
  useFetchAccountInfo,
  useAccountInfo,
} from "providers/accounts";
import { StakeAccountSection } from "components/account/StakeAccountSection";
import { TokenAccountSection } from "components/account/TokenAccountSection";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { useCluster, ClusterStatus } from "providers/cluster";
import { NavLink } from "react-router-dom";
import { clusterPath } from "utils/url";
import { UnknownAccountCard } from "components/account/UnknownAccountCard";
import { OwnedTokensCard } from "components/account/OwnedTokensCard";
import { TransactionHistoryCard } from "components/account/TransactionHistoryCard";

type Props = { address: string; tab?: string };
export function AccountDetailsPage({ address, tab }: Props) {
  let pubkey: PublicKey | undefined;
  try {
    pubkey = new PublicKey(address);
  } catch (err) {
    console.error(err);
    // TODO handle bad addresses
  }

  let moreTab: MoreTabs = "history";
  if (tab === "history" || tab === "tokens") {
    moreTab = tab;
  }

  return (
    <div className="container mt-n3">
      <div className="header">
        <div className="header-body">
          <h6 className="header-pretitle">Details</h6>
          <h4 className="header-title">Account</h4>
        </div>
      </div>
      {pubkey && <InfoSection pubkey={pubkey} />}
      {pubkey && <MoreSection pubkey={pubkey} tab={moreTab} />}
    </div>
  );
}

function InfoSection({ pubkey }: { pubkey: PublicKey }) {
  const fetchAccount = useFetchAccountInfo();
  const address = pubkey.toBase58();
  const info = useAccountInfo(address);
  const refresh = useFetchAccountInfo();
  const { status } = useCluster();

  // Fetch account on load
  React.useEffect(() => {
    if (!info && status === ClusterStatus.Connected) fetchAccount(pubkey);
  }, [address, status]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!info || info.status === FetchStatus.Fetching) {
    return <LoadingCard />;
  } else if (
    info.status === FetchStatus.FetchFailed ||
    info.lamports === undefined
  ) {
    return <ErrorCard retry={() => refresh(pubkey)} text="Fetch Failed" />;
  }

  const data = info.details?.data;
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
        account={info}
        stakeAccount={stakeAccount}
        stakeAccountType={stakeAccountType}
      />
    );
  } else if (data && data.name === "spl-token") {
    return <TokenAccountSection account={info} tokenAccount={data.parsed} />;
  } else {
    return <UnknownAccountCard account={info} />;
  }
}

type MoreTabs = "history" | "tokens";
function MoreSection({ pubkey, tab }: { pubkey: PublicKey; tab: MoreTabs }) {
  const address = pubkey.toBase58();
  const info = useAccountInfo(address);
  if (!info || info.lamports === undefined) return null;

  return (
    <>
      <div className="container">
        <div className="header">
          <div className="header-body pt-0">
            <ul className="nav nav-tabs nav-overflow header-tabs">
              <li className="nav-item">
                <NavLink
                  className="nav-link"
                  to={clusterPath(`/address/${address}`)}
                  exact
                >
                  History
                </NavLink>
              </li>
              <li className="nav-item">
                <NavLink
                  className="nav-link"
                  to={clusterPath(`/address/${address}/tokens`)}
                  exact
                >
                  Tokens
                </NavLink>
              </li>
            </ul>
          </div>
        </div>
      </div>
      {tab === "tokens" && <OwnedTokensCard pubkey={pubkey} />}
      {tab === "history" && <TransactionHistoryCard pubkey={pubkey} />}
    </>
  );
}
