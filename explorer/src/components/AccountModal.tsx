import React from "react";
import {
  useSelectedAccount,
  useAccountsDispatch,
  ActionType,
  Account,
  Status
} from "../providers/accounts";
import { TransactionError } from "@solana/web3.js";
import Copyable from "./Copyable";
import Overlay from "./Overlay";

function AccountModal() {
  const selected = useSelectedAccount();
  const dispatch = useAccountsDispatch();
  const onClose = () => dispatch({ type: ActionType.Select });
  const show = !!selected;

  const renderContent = () => {
    if (!selected) return null;
    return (
      <div className="modal-dialog modal-dialog-centered">
        <div className="modal-content" onClick={e => e.stopPropagation()}>
          <div className="modal-card card">
            <div className="card-header">
              <h4 className="card-header-title">Account Transaction History</h4>
              <button type="button" className="close" onClick={onClose}>
                <span aria-hidden="true">&times;</span>
              </button>
            </div>
            <div className="card-body">
              <AccountDetails account={selected} />
            </div>
          </div>
        </div>
      </div>
    );
  };

  return (
    <>
      <div className={`modal fade${show ? " show" : ""}`} onClick={onClose}>
        {renderContent()}
      </div>
      <Overlay show={show} />
    </>
  );
}

function AccountDetails({ account }: { account: Account }) {
  const renderError = (content: React.ReactNode) => {
    return <span className="text-info">{content}</span>;
  };

  if (account.status === Status.FetchingHistory) {
    return renderError(
      <>
        <span className="spinner-grow spinner-grow-sm mr-2"></span>
        Loading
      </>
    );
  }

  if (account.history === undefined)
    return renderError("Failed to fetch account transaction history");

  if (account.history.size === 0) return renderError("No transactions found");

  const detailsList: React.ReactNode[] = [];
  account.history.forEach((slotTransactions, slot) => {
    detailsList.push(
      <SlotTransactionDetails
        slot={slot}
        statuses={Array.from(slotTransactions.entries())}
      />
    );
  });

  return (
    <>
      {detailsList.map((details, i) => {
        return (
          <React.Fragment key={++i}>
            {i > 1 ? <hr className="mt-0 mx-n4 mb-4"></hr> : null}
            {details}
          </React.Fragment>
        );
      })}
    </>
  );
}

function SlotTransactionDetails({
  slot,
  statuses
}: {
  slot: number;
  statuses: [string, TransactionError | null][];
}) {
  return (
    <>
      <h4 className="slot-pill">Slot #{slot}</h4>
      <div className="list-group list-group-flush">
        {statuses.map(([signature, err]) => {
          return (
            <ListGroupItem
              key={signature}
              signature={signature}
              failed={err !== null}
            />
          );
        })}
      </div>
    </>
  );
}

function ListGroupItem({
  signature,
  failed
}: {
  signature: string;
  failed: boolean;
}) {
  let badgeText, badgeColor;
  if (failed) {
    badgeText = "Error";
    badgeColor = "danger";
  } else {
    badgeText = "Success";
    badgeColor = "primary";
  }

  return (
    <div className="list-group-item slot-item">
      <div className="row align-items-center justify-content-between flex-nowrap">
        <div className="col-auto">
          <span className={`badge badge-soft-${badgeColor} badge-pill`}>
            {badgeText}
          </span>
        </div>
        <div className="col min-width-0">
          <Copyable text={signature}>
            <h5 className="mb-0 text-truncate">
              <code>{signature}</code>
            </h5>
          </Copyable>
        </div>
      </div>
    </div>
  );
}

export default AccountModal;
