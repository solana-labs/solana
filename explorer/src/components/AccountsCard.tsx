import React from "react";
import { Link } from "react-router-dom";
import {
  useAccounts,
  Account,
  Status,
  useFetchAccountInfo
} from "../providers/accounts";
import { assertUnreachable } from "../utils";
import { displayAddress } from "../utils/tx";
import { PublicKey } from "@solana/web3.js";
import Copyable from "./Copyable";
import { lamportsToSolString } from "utils";

function AccountsCard() {
  const { accounts, idCounter } = useAccounts();
  const fetchAccountInfo = useFetchAccountInfo();
  const addressInput = React.useRef<HTMLInputElement>(null);
  const [error, setError] = React.useState("");

  const onNew = (address: string) => {
    if (address.length === 0) return;
    let pubkey;
    try {
      pubkey = new PublicKey(address);
    } catch (err) {
      setError(`${err}`);
      return;
    }

    fetchAccountInfo(pubkey);
    const inputEl = addressInput.current;
    if (inputEl) {
      inputEl.value = "";
    }
  };

  return (
    <div className="card">
      {renderHeader()}

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">
                <span className="fe fe-hash"></span>
              </th>
              <th className="text-muted">Status</th>
              <th className="text-muted">Address</th>
              <th className="text-muted">Balance (SOL)</th>
              <th className="text-muted">Data (bytes)</th>
              <th className="text-muted">Owner</th>
              <th className="text-muted">Details</th>
            </tr>
          </thead>
          <tbody className="list">
            <tr>
              <td>
                <span className="badge badge-soft-dark badge-pill">
                  {idCounter + 1}
                </span>
              </td>
              <td>
                <span className={`badge badge-soft-dark`}>New</span>
              </td>
              <td>
                <input
                  type="text"
                  onInput={() => setError("")}
                  onKeyDown={e =>
                    e.keyCode === 13 && onNew(e.currentTarget.value)
                  }
                  onSubmit={e => onNew(e.currentTarget.value)}
                  ref={addressInput}
                  className={`form-control text-address text-monospace ${
                    error ? "is-invalid" : ""
                  }`}
                  placeholder="input account address"
                />
                {error ? <div className="invalid-feedback">{error}</div> : null}
              </td>
              <td>-</td>
              <td>-</td>
              <td>-</td>
              <td></td>
            </tr>
            {accounts.map(account => renderAccountRow(account))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

const renderHeader = () => {
  return (
    <div className="card-header">
      <div className="row align-items-center">
        <div className="col">
          <h4 className="card-header-title">Look Up Account(s)</h4>
        </div>
      </div>
    </div>
  );
};

const renderAccountRow = (account: Account) => {
  let statusText;
  let statusClass;
  switch (account.status) {
    case Status.NotFound:
      statusClass = "danger";
      statusText = "Not Found";
      break;
    case Status.CheckFailed:
    case Status.HistoryFailed:
      statusClass = "dark";
      statusText = "Cluster Error";
      break;
    case Status.Checking:
    case Status.FetchingHistory:
      statusClass = "info";
      statusText = "Fetching";
      break;
    case Status.Success:
      if (account.details?.executable) {
        statusClass = "dark";
        statusText = "Executable";
      } else {
        statusClass = "success";
        statusText = "Found";
      }
      break;
    default:
      return assertUnreachable(account.status);
  }

  let data = "-";
  let owner = "-";
  if (account.details) {
    data = `${account.details.space}`;
    owner = displayAddress(account.details.owner);
  }

  let balance = "-";
  if (account.lamports !== undefined) {
    balance = lamportsToSolString(account.lamports);
  }

  const base58AccountPubkey = account.pubkey.toBase58();
  return (
    <tr key={account.id}>
      <td>
        <span className="badge badge-soft-dark badge-pill">{account.id}</span>
      </td>
      <td>
        <span className={`badge badge-soft-${statusClass}`}>{statusText}</span>
      </td>
      <td>
        <Copyable text={base58AccountPubkey}>
          <code>{base58AccountPubkey}</code>
        </Copyable>
      </td>
      <td>{balance}</td>
      <td>{data}</td>
      <td>
        {owner === "-" ? (
          owner
        ) : (
          <Copyable text={owner}>
            <code>{owner}</code>
          </Copyable>
        )}
      </td>
      <td>
        <Link
          to={location => ({
            ...location,
            pathname: "/account/" + base58AccountPubkey
          })}
          className="btn btn-rounded-circle btn-white btn-sm"
        >
          <span className="fe fe-arrow-right"></span>
        </Link>
      </td>
    </tr>
  );
};

export default AccountsCard;
