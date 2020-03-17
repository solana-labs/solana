import React from "react";
import {
  useTransactions,
  Transaction,
  Status
} from "../providers/transactions";

function TransactionsCard() {
  const { transactions } = useTransactions();

  return (
    <div className="card">
      {renderHeader()}

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">Status</th>
              <th className="text-muted">Signature</th>
              <th className="text-muted">Confirmations</th>
              <th className="text-muted">Slot Number</th>
            </tr>
          </thead>
          <tbody className="list">
            {Object.values(transactions).map(transaction =>
              renderTransactionRow(transaction)
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
          <h4 className="card-header-title">Transactions</h4>
        </div>
      </div>
    </div>
  );
};

const renderTransactionRow = (transaction: Transaction) => {
  let statusText;
  let statusClass;
  switch (transaction.status) {
    case Status.CheckFailed:
      statusClass = "dark";
      statusText = "Network Error";
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
    case Status.Pending:
      statusClass = "warning";
      statusText = "Pending";
      break;
  }

  return (
    <tr key={transaction.signature}>
      <td>
        <span className={`badge badge-soft-${statusClass}`}>{statusText}</span>
      </td>
      <td>
        <code>{transaction.signature}</code>
      </td>
      <td>TODO</td>
      <td>TODO</td>
    </tr>
  );
};

export default TransactionsCard;
