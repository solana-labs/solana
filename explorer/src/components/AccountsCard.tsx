import React from "react";
import {
  useAccounts,
  useAccountsDispatch,
  Dispatch,
  fetchAccountInfo,
  ActionType,
  Account,
  Status
} from "../providers/accounts";
import { assertUnreachable } from "../utils";
import { displayAddress } from "../utils/tx";
import { useCluster } from "../providers/cluster";
import { PublicKey, LAMPORTS_PER_SOL } from "@solana/web3.js";
import Copyable from "./Copyable";

function AccountsCard() {
  const { accounts, idCounter } = useAccounts();
  const dispatch = useAccountsDispatch();
  const addressInput = React.useRef<HTMLInputElement>(null);
  const [error, setError] = React.useState("");
  const { url } = useCluster();

  const onNew = (address: string) => {
    if (address.length === 0) return;
    let pubkey;
    try {
      pubkey = new PublicKey(address);
    } catch (err) {
      setError(`${err}`);
      return;
    }

    dispatch({ type: ActionType.Input, pubkey });
    fetchAccountInfo(dispatch, address, url);

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
            {accounts.map(account => renderAccountRow(account, dispatch, url))}
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

const renderAccountRow = (
  account: Account,
  dispatch: Dispatch,
  url: string
) => {
  let statusText;
  let statusClass;
  switch (account.status) {
    case Status.NotFound:
      statusClass = "danger";
      statusText = "Not Found";
      break;
    case Status.CheckFailed:
    case Status.HistoryFailed:
      statusClass = "danger";
      statusText = "Error";
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
    balance = `◎${(1.0 * account.lamports) / LAMPORTS_PER_SOL}`;
  }

  const renderDetails = () => {
    let onClick, icon;
    switch (account.status) {
      case Status.Success:
        icon = "more-horizontal";
        onClick = () =>
          dispatch({
            type: ActionType.Select,
            address: account.pubkey.toBase58()
          });
        break;

      case Status.CheckFailed:
      case Status.HistoryFailed: {
        icon = "refresh-cw";
        onClick = () => {
          fetchAccountInfo(dispatch, account.pubkey.toBase58(), url);
        };
        break;
      }

      default: {
        return null;
      }
    }

    return (
      <button
        className="btn btn-rounded-circle btn-white btn-sm"
        onClick={onClick}
      >
        <span className={`fe fe-${icon}`}></span>
      </button>
    );
  };

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
      <td>{renderDetails()}</td>
    </tr>
  );
};

export default AccountsCard;
