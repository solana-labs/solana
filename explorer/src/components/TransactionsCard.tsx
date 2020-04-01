import React from "react";
import {
  useTransactions,
  useTransactionsDispatch,
  checkTransactionStatus,
  ActionType,
  Transaction,
  Status
} from "../providers/transactions";
import bs58 from "bs58";
import { assertUnreachable } from "../utils";
import { useCluster } from "../providers/cluster";

function TransactionsCard() {
  const { transactions, idCounter } = useTransactions();
  const dispatch = useTransactionsDispatch();
  const signatureInput = React.useRef<HTMLInputElement>(null);
  const [error, setError] = React.useState("");
  const { url } = useCluster();

  const onNew = (signature: string) => {
    if (signature.length === 0) return;
    try {
      const length = bs58.decode(signature).length;
      if (length > 64) {
        setError("Signature is too long");
        return;
      } else if (length < 64) {
        setError("Signature is too short");
        return;
      }
    } catch (err) {
      setError(`${err}`);
      return;
    }

    dispatch({ type: ActionType.InputSignature, signature });
    checkTransactionStatus(dispatch, signature, url);

    const inputEl = signatureInput.current;
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
              <th className="text-muted">Signature</th>
              <th className="text-muted">Confirmations</th>
              <th className="text-muted">Slot Number</th>
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
                  ref={signatureInput}
                  className={`form-control text-signature text-monospace ${
                    error ? "is-invalid" : ""
                  }`}
                  placeholder="input transaction signature"
                />
                {error ? <div className="invalid-feedback">{error}</div> : null}
              </td>
              <td>-</td>
              <td>-</td>
              <td></td>
            </tr>
            {transactions.map(transaction =>
              renderTransactionRow(transaction, dispatch, url)
            )}
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
          <h4 className="card-header-title">Look Up Transaction(s)</h4>
        </div>
      </div>
    </div>
  );
};

const renderTransactionRow = (
  transaction: Transaction,
  dispatch: any,
  url: string
) => {
  let statusText;
  let statusClass;
  switch (transaction.status) {
    case Status.CheckFailed:
      statusClass = "dark";
      statusText = "Cluster Error";
      break;
    case Status.Checking:
      statusClass = "info";
      statusText = "Checking";
      break;
    case Status.Success:
      statusClass = "success";
      statusText = "Success";
      break;
    case Status.Failure:
      statusClass = "danger";
      statusText = "Failed";
      break;
    case Status.Missing:
      statusClass = "warning";
      statusText = "Not Found";
      break;
    default:
      return assertUnreachable(transaction.status);
  }

  const slotText = `${transaction.slot || "-"}`;
  const confirmationsText = `${transaction.confirmations || "-"}`;

  const renderDetails = () => {
    let onClick, icon;
    if (transaction.confirmations === "max") {
      icon = "more-horizontal";
      onClick = () =>
        dispatch({
          type: ActionType.Select,
          signature: transaction.signature
        });
    } else {
      icon = "refresh-cw";
      onClick = () => {
        checkTransactionStatus(dispatch, transaction.signature, url);
      };
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

  return (
    <tr key={transaction.signature}>
      <td>
        <span className="badge badge-soft-dark badge-pill">
          {transaction.id}
        </span>
      </td>
      <td>
        <span className={`badge badge-soft-${statusClass}`}>{statusText}</span>
      </td>
      <td>
        <code>{transaction.signature}</code>
      </td>
      <td className="text-uppercase">{confirmationsText}</td>
      <td>{slotText}</td>
      <td>{renderDetails()}</td>
    </tr>
  );
};

export default TransactionsCard;
