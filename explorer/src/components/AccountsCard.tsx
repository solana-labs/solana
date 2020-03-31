import React from "react";
import {
  useAccounts,
  useAccountsDispatch,
  fetchAccountInfo,
  ActionType,
  Account,
  Status
} from "../providers/accounts";
import { assertUnreachable } from "../utils";
import { useCluster } from "../providers/cluster";
import { PublicKey, LAMPORTS_PER_SOL } from "@solana/web3.js";

function AccountsCard() {
  const { accounts, idCounter } = useAccounts();
  const dispatch = useAccountsDispatch();
  const addressInput = React.useRef<HTMLInputElement>(null);
  const [error, setError] = React.useState("");
  const [showSOL, setShowSOL] = React.useState(true);
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
    fetchAccountInfo(dispatch, idCounter + 1, pubkey, url);

    const inputEl = addressInput.current;
    if (inputEl) {
      inputEl.value = "";
    }
  };

  return (
    <div className="card">
      {renderHeader(showSOL, setShowSOL)}

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">
                <span className="fe fe-hash"></span>
              </th>
              <th className="text-muted">Status</th>
              <th className="text-muted">Address</th>
              <th className="text-muted">
                Balance ({showSOL ? "SOL" : "lamports"})
              </th>
              <th className="text-muted">Data (bytes)</th>
              <th className="text-muted">Owner</th>
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
            </tr>
            {accounts.map(account => renderAccountRow(account, showSOL))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

const renderHeader = (
  showSOL: boolean,
  setShowSOL: (show: boolean) => void
) => {
  return (
    <div className="card-header">
      <div className="row align-items-center">
        <div className="col">
          <h4 className="card-header-title">Look Up Account(s)</h4>
        </div>

        <span className="text-muted mr-3">Display SOL</span>

        <div className="custom-control custom-switch">
          <input
            type="checkbox"
            className="custom-control-input"
            checked={showSOL}
          />
          <label
            className="custom-control-label"
            onClick={() => setShowSOL(!showSOL)}
          ></label>
        </div>
      </div>
    </div>
  );
};

const renderAccountRow = (account: Account, showSOL: boolean) => {
  let statusText;
  let statusClass;
  switch (account.status) {
    case Status.CheckFailed:
      statusClass = "danger";
      statusText = "Error";
      break;
    case Status.Checking:
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
  let balance = "-";
  let owner = "-";
  if (account.details) {
    data = `${account.details.space}`;
    if (showSOL) {
      balance = `${(1.0 * account.details.lamports) / LAMPORTS_PER_SOL}`;
    } else {
      balance = `${account.details.lamports}`;
    }
    owner = `${account.details.owner.toBase58()}`;
  }

  return (
    <tr key={account.id}>
      <td>
        <span className="badge badge-soft-dark badge-pill">{account.id}</span>
      </td>
      <td>
        <span className={`badge badge-soft-${statusClass}`}>{statusText}</span>
      </td>
      <td>
        <code>{account.pubkey.toBase58()}</code>
      </td>
      <td>{balance}</td>
      <td>{data}</td>
      <td>{owner === "-" ? owner : <code>{owner}</code>}</td>
    </tr>
  );
};

export default AccountsCard;
