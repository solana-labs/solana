import React from "react";
import { PublicKey, StakeProgram } from "@solana/web3.js";
import {
  FetchStatus,
  useFetchAccountInfo,
  useAccountInfo,
  useAccountHistory,
  Account,
} from "providers/accounts";
import { lamportsToSolString } from "utils";
import { StakeAccountCards } from "components/account/StakeAccountCards";
import ErrorCard from "components/common/ErrorCard";
import LoadingCard from "components/common/LoadingCard";
import TableCardBody from "components/common/TableCardBody";
import { useFetchAccountHistory } from "providers/accounts/history";
import {
  useFetchAccountOwnedTokens,
  useAccountOwnedTokens,
  TokenAccountData,
} from "providers/accounts/tokens";
import { useCluster, ClusterStatus } from "providers/cluster";
import Address from "./common/Address";
import Signature from "./common/Signature";

type Props = { address: string };
export default function AccountDetails({ address }: Props) {
  let pubkey: PublicKey | undefined;
  try {
    pubkey = new PublicKey(address);
  } catch (err) {
    console.error(err);
    // TODO handle bad addresses
  }

  return (
    <div className="container mt-n3">
      <div className="header">
        <div className="header-body">
          <h6 className="header-pretitle">Details</h6>
          <h4 className="header-title">Account</h4>
        </div>
      </div>
      {pubkey && <AccountCards pubkey={pubkey} />}
      {pubkey && <TokensCard pubkey={pubkey} />}
      {pubkey && <HistoryCard pubkey={pubkey} />}
    </div>
  );
}

function AccountCards({ pubkey }: { pubkey: PublicKey }) {
  const fetchAccount = useFetchAccountInfo();
  const address = pubkey.toBase58();
  const info = useAccountInfo(address);
  const refresh = useFetchAccountInfo();
  const { status } = useCluster();

  // Fetch account on load
  React.useEffect(() => {
    if (pubkey && !info && status === ClusterStatus.Connected)
      fetchAccount(pubkey);
  }, [address, status]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!info || info.status === FetchStatus.Fetching) {
    return <LoadingCard />;
  } else if (
    info.status === FetchStatus.FetchFailed ||
    info.lamports === undefined
  ) {
    return <ErrorCard retry={() => refresh(pubkey)} text="Fetch Failed" />;
  }

  const owner = info.details?.owner;
  const data = info.details?.data;
  if (data && owner && owner.equals(StakeProgram.programId)) {
    return <StakeAccountCards account={info} stakeAccount={data} />;
  } else {
    return <UnknownAccountCard account={info} />;
  }
}

function UnknownAccountCard({ account }: { account: Account }) {
  const { details, lamports } = account;
  if (lamports === undefined) return null;

  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Overview</h3>
      </div>

      <TableCardBody>
        <tr>
          <td>Address</td>
          <td className="text-lg-right">
            <Address pubkey={account.pubkey} alignRight />
          </td>
        </tr>
        <tr>
          <td>Balance (SOL)</td>
          <td className="text-lg-right text-uppercase">
            {lamportsToSolString(lamports)}
          </td>
        </tr>

        {details && (
          <tr>
            <td>Data (Bytes)</td>
            <td className="text-lg-right">{details.space}</td>
          </tr>
        )}

        {details && (
          <tr>
            <td>Owner</td>
            <td className="text-lg-right">
              <Address pubkey={details.owner} alignRight link />
            </td>
          </tr>
        )}

        {details && (
          <tr>
            <td>Executable</td>
            <td className="text-lg-right">
              {details.executable ? "Yes" : "No"}
            </td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}

function TokensCard({ pubkey }: { pubkey: PublicKey }) {
  const address = pubkey.toBase58();
  const ownedTokens = useAccountOwnedTokens(address);
  const fetchAccountTokens = useFetchAccountOwnedTokens();
  const refresh = () => fetchAccountTokens(pubkey);

  if (ownedTokens === undefined) {
    return null;
  }

  const { status, tokens } = ownedTokens;
  const fetching = status === FetchStatus.Fetching;
  if (fetching && (tokens === undefined || tokens.length === 0)) {
    return <LoadingCard message="Loading owned tokens" />;
  } else if (tokens === undefined) {
    return <ErrorCard retry={refresh} text="Failed to fetch owned tokens" />;
  }

  if (tokens.length === 0) {
    return (
      <ErrorCard
        retry={refresh}
        retryText="Try Again"
        text={"No owned tokens found"}
      />
    );
  }

  const mappedTokens = new Map<string, TokenAccountData>();
  for (const token of tokens) {
    const mintAddress = token.mint.toBase58();
    const tokenInfo = mappedTokens.get(mintAddress);
    if (tokenInfo) {
      tokenInfo.amount += token.amount;
    } else {
      mappedTokens.set(mintAddress, token);
    }
  }

  const detailsList: React.ReactNode[] = [];
  mappedTokens.forEach((tokenInfo, mintAddress) => {
    const balance = tokenInfo.amount;
    detailsList.push(
      <tr key={mintAddress}>
        <td>
          <Address pubkey={new PublicKey(mintAddress)} link />
        </td>
        <td>{balance}</td>
      </tr>
    );
  });

  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Tokens</h3>
        <button
          className="btn btn-white btn-sm"
          disabled={fetching}
          onClick={refresh}
        >
          {fetching ? (
            <>
              <span className="spinner-grow spinner-grow-sm mr-2"></span>
              Loading
            </>
          ) : (
            <>
              <span className="fe fe-refresh-cw mr-2"></span>
              Refresh
            </>
          )}
        </button>
      </div>

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">Token Address</th>
              <th className="text-muted">Balance</th>
            </tr>
          </thead>
          <tbody className="list">{detailsList}</tbody>
        </table>
      </div>
    </div>
  );
}

function HistoryCard({ pubkey }: { pubkey: PublicKey }) {
  const address = pubkey.toBase58();
  const info = useAccountInfo(address);
  const history = useAccountHistory(address);
  const fetchAccountHistory = useFetchAccountHistory();
  const refresh = () => fetchAccountHistory(pubkey, true);
  const loadMore = () => fetchAccountHistory(pubkey);

  if (!info || !history || info.lamports === undefined) {
    return null;
  } else if (
    history.fetched === undefined ||
    history.fetchedRange === undefined
  ) {
    if (history.status === FetchStatus.Fetching) {
      return <LoadingCard message="Loading history" />;
    }

    return (
      <ErrorCard retry={refresh} text="Failed to fetch transaction history" />
    );
  }

  if (history.fetched.length === 0) {
    if (history.status === FetchStatus.Fetching) {
      return <LoadingCard message="Loading history" />;
    }
    return (
      <ErrorCard
        retry={loadMore}
        retryText="Look back further"
        text={
          "No transaction history found since slot " + history.fetchedRange.min
        }
      />
    );
  }

  const detailsList: React.ReactNode[] = [];
  const transactions = history.fetched;

  for (var i = 0; i < transactions.length; i++) {
    const slot = transactions[i].status.slot;
    const slotTransactions = [transactions[i]];
    while (i + 1 < transactions.length) {
      const nextSlot = transactions[i + 1].status.slot;
      if (nextSlot !== slot) break;
      slotTransactions.push(transactions[++i]);
    }

    slotTransactions.forEach(({ signature, status }, index) => {
      let statusText;
      let statusClass;
      if (status.err) {
        statusClass = "warning";
        statusText = "Failed";
      } else {
        statusClass = "success";
        statusText = "Success";
      }

      detailsList.push(
        <tr key={signature}>
          <td className="w-1">{slot}</td>

          <td>
            <span className={`badge badge-soft-${statusClass}`}>
              {statusText}
            </span>
          </td>

          <td>
            <Signature signature={signature} link />
          </td>
        </tr>
      );
    });
  }

  const fetching = history.status === FetchStatus.Fetching;
  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Transaction History</h3>
        <button
          className="btn btn-white btn-sm"
          disabled={fetching}
          onClick={refresh}
        >
          {fetching ? (
            <>
              <span className="spinner-grow spinner-grow-sm mr-2"></span>
              Loading
            </>
          ) : (
            <>
              <span className="fe fe-refresh-cw mr-2"></span>
              Refresh
            </>
          )}
        </button>
      </div>

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted w-1">Slot</th>
              <th className="text-muted">Result</th>
              <th className="text-muted">Transaction Signature</th>
            </tr>
          </thead>
          <tbody className="list">{detailsList}</tbody>
        </table>
      </div>

      <div className="card-footer">
        <button
          className="btn btn-primary w-100"
          onClick={loadMore}
          disabled={fetching}
        >
          {fetching ? (
            <>
              <span className="spinner-grow spinner-grow-sm mr-2"></span>
              Loading
            </>
          ) : (
            "Load More"
          )}
        </button>
      </div>
    </div>
  );
}
