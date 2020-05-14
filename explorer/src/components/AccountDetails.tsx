import React from "react";
import { useClusterModal } from "providers/cluster";
import { PublicKey, StakeProgram } from "@solana/web3.js";
import ClusterStatusButton from "components/ClusterStatusButton";
import { useHistory, useLocation } from "react-router-dom";
import {
  FetchStatus,
  useFetchAccountInfo,
  useAccountInfo,
  useAccountHistory,
  Account
} from "providers/accounts";
import { lamportsToSolString } from "utils";
import Copyable from "./Copyable";
import { displayAddress } from "utils/tx";
import { StakeAccountCards } from "components/account/StakeAccountCards";
import ErrorCard from "components/common/ErrorCard";
import LoadingCard from "components/common/LoadingCard";
import TableCardBody from "components/common/TableCardBody";
import { useFetchAccountHistory } from "providers/accounts/history";

type Props = { address: string };
export default function AccountDetails({ address }: Props) {
  const fetchAccount = useFetchAccountInfo();
  const [, setShow] = useClusterModal();
  const [search, setSearch] = React.useState(address);
  const history = useHistory();
  const location = useLocation();

  let pubkey: PublicKey | undefined;
  try {
    pubkey = new PublicKey(address);
  } catch (err) {
    console.error(err);
    // TODO handle bad addresses
  }

  const updateAddress = () => {
    history.push({ ...location, pathname: "/account/" + search });
  };

  // Fetch account on load
  React.useEffect(() => {
    setSearch(address);
    if (pubkey) fetchAccount(pubkey);
  }, [address]); // eslint-disable-line react-hooks/exhaustive-deps

  const searchInput = (
    <input
      type="text"
      value={search}
      onChange={e => setSearch(e.target.value)}
      onKeyUp={e => e.key === "Enter" && updateAddress()}
      className="form-control form-control-prepended search text-monospace"
      placeholder="Search for address"
    />
  );

  return (
    <div className="container">
      <div className="header">
        <div className="header-body">
          <div className="row align-items-center">
            <div className="col">
              <h6 className="header-pretitle">Details</h6>
              <h3 className="header-title">Account</h3>
            </div>
            <div className="col-auto">
              <ClusterStatusButton onClick={() => setShow(true)} />
            </div>
          </div>
        </div>
      </div>

      <div className="row mb-4 mt-n2 align-items-center">
        <div className="col d-none d-md-block">
          <div className="input-group input-group-merge">
            {searchInput}
            <div className="input-group-prepend">
              <div className="input-group-text">
                <span className="fe fe-search"></span>
              </div>
            </div>
          </div>
        </div>
        <div className="col d-block d-md-none">{searchInput}</div>
        <div className="col-auto ml-n3 d-block d-md-none">
          <button className="btn btn-white" onClick={updateAddress}>
            <span className="fe fe-search"></span>
          </button>
        </div>
      </div>
      {pubkey && <AccountCards pubkey={pubkey} />}
      {pubkey && <HistoryCard pubkey={pubkey} />}
    </div>
  );
}

function AccountCards({ pubkey }: { pubkey: PublicKey }) {
  const address = pubkey.toBase58();
  const info = useAccountInfo(address);
  const refresh = useFetchAccountInfo();

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
        <h3 className="card-header-title">Account Overview</h3>
      </div>

      <TableCardBody>
        <tr>
          <td>Balance (SOL)</td>
          <td className="text-right text-uppercase">
            {lamportsToSolString(lamports)}
          </td>
        </tr>

        {details && (
          <tr>
            <td>Data (Bytes)</td>
            <td className="text-right">{details.space}</td>
          </tr>
        )}

        {details && (
          <tr>
            <td>Owner</td>
            <td className="text-right">
              <Copyable text={details.owner.toBase58()}>
                <code>{displayAddress(details.owner.toBase58())}</code>
              </Copyable>
            </td>
          </tr>
        )}

        {details && (
          <tr>
            <td>Executable</td>
            <td className="text-right">{details.executable ? "Yes" : "No"}</td>
          </tr>
        )}
      </TableCardBody>
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
      return <LoadingCard />;
    }

    return (
      <ErrorCard retry={refresh} text="Failed to fetch transaction history" />
    );
  }

  if (history.fetched.length === 0) {
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
    const signatures = slotTransactions.map(({ signature, status }) => {
      return (
        <code key={signature} className="mb-2 mb-last-0">
          {signature}
        </code>
      );
    });

    detailsList.push(
      <tr key={slot}>
        <td className="vertical-top">Slot {slot}</td>
        <td className="text-right">
          <div className="d-inline-flex flex-column align-items-end">
            {signatures}
          </div>
        </td>
      </tr>
    );
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

      <TableCardBody>{detailsList}</TableCardBody>
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
