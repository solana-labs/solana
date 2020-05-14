import React from "react";
import { useClusterModal } from "providers/cluster";
import { PublicKey, StakeProgram } from "@solana/web3.js";
import ClusterStatusButton from "components/ClusterStatusButton";
import { useHistory, useLocation } from "react-router-dom";
import {
  Status,
  useFetchAccountInfo,
  useFetchAccountHistory,
  useAccountInfo,
  Account
} from "providers/accounts";
import { lamportsToSolString } from "utils";
import Copyable from "./Copyable";
import { displayAddress } from "utils/tx";
import { StakeAccountCards } from "components/account/StakeAccountCards";
import ErrorCard from "components/common/ErrorCard";
import LoadingCard from "components/common/LoadingCard";
import TableCardBody from "components/common/TableCardBody";

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
    if (pubkey) fetchAccount(pubkey);
  }, [pubkey?.toBase58()]); // eslint-disable-line react-hooks/exhaustive-deps

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

  if (!info || info.status === Status.Checking) {
    return <LoadingCard />;
  } else if (
    info.status === Status.CheckFailed ||
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
  const refresh = useFetchAccountInfo();

  const { details, lamports, pubkey } = account;
  if (lamports === undefined) return null;

  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Account Overview</h3>
        <button
          className="btn btn-white btn-sm"
          onClick={() => refresh(pubkey)}
        >
          <span className="fe fe-refresh-cw mr-2"></span>
          Refresh
        </button>
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
  const refresh = useFetchAccountHistory();

  if (!info || !info.details) {
    return null;
  } else if (info.status === Status.FetchingHistory) {
    return <LoadingCard />;
  } else if (info.history === undefined) {
    return (
      <ErrorCard
        retry={() => refresh(pubkey)}
        text="Failed to fetch transaction history"
      />
    );
  }

  if (info.history.size === 0) {
    return (
      <ErrorCard
        retry={() => refresh(pubkey)}
        text="No recent transaction history found"
      />
    );
  }

  const detailsList: React.ReactNode[] = [];
  info.history.forEach((slotTransactions, slot) => {
    const signatures = Array.from(slotTransactions.entries()).map(
      ([signature, err]) => {
        return <code className="mb-2 mb-last-0">{signature}</code>;
      }
    );

    detailsList.push(
      <tr>
        <td className="vertical-top">Slot {slot}</td>
        <td className="text-right">
          <div className="d-inline-flex flex-column align-items-end">
            {signatures}
          </div>
        </td>
      </tr>
    );
  });

  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Transaction History</h3>
        <button
          className="btn btn-white btn-sm"
          onClick={() => refresh(pubkey)}
        >
          <span className="fe fe-refresh-cw mr-2"></span>
          Refresh
        </button>
      </div>

      <TableCardBody>{detailsList}</TableCardBody>
    </div>
  );
}
