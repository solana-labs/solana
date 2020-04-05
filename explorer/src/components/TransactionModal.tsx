import React from "react";
import {
  useTransactions,
  useTransactionsDispatch,
  ActionType,
  Selected
} from "../providers/transactions";
import { useBlocks } from "../providers/blocks";
import { LAMPORTS_PER_SOL } from "@solana/web3.js";

function TransactionModal() {
  const { selected } = useTransactions();
  const dispatch = useTransactionsDispatch();
  const onClose = () => dispatch({ type: ActionType.Deselect });
  const show = !!selected;

  const renderContent = () => {
    if (!selected) return null;
    return (
      <div className="modal-dialog modal-dialog-centered">
        <div className="modal-content" onClick={e => e.stopPropagation()}>
          <div className="modal-card card">
            <div className="card-header">
              <h4 className="card-header-title">Transaction Details</h4>

              <button type="button" className="close" onClick={onClose}>
                <span aria-hidden="true">&times;</span>
              </button>
            </div>

            <TransactionDetails selected={selected} />
          </div>
        </div>
      </div>
    );
  };

  return (
    <div className={`modal fade${show ? " show" : ""}`} onClick={onClose}>
      {renderContent()}
    </div>
  );
}

function TransactionDetails({ selected }: { selected: Selected }) {
  const { blocks } = useBlocks();
  const block = blocks[selected.slot];
  if (!block)
    return <span className="text-info">{"Transaction block not found"}</span>;

  if (!block.transactions) {
    return (
      <span className="text-info">
        <span className="spinner-grow spinner-grow-sm mr-2"></span>
        Loading
      </span>
    );
  }

  const details = block.transactions[selected.signature];
  if (!details)
    return <span className="text-info">{"Transaction not found"}</span>;

  if (details.transfers.length === 0)
    return <span className="text-info">{"No transfers"}</span>;

  let i = 0;
  return (
    <>
      {details.transfers.map(transfer => {
        return (
          <div key={++i}>
            {i > 1 ? <hr className="mb-4"></hr> : null}
            <div className="card-body">
              <div className="list-group list-group-flush my-n3">
                <div className="list-group-item">
                  <div className="row align-items-center">
                    <div className="col">
                      <h5 className="mb-0">From</h5>
                    </div>
                    <div className="col-auto">
                      <code>{transfer.fromPubkey.toBase58()}</code>
                    </div>
                  </div>
                </div>

                <div className="list-group-item">
                  <div className="row align-items-center">
                    <div className="col">
                      <h5 className="mb-0">To</h5>
                    </div>
                    <div className="col-auto">
                      <code>{transfer.toPubkey.toBase58()}</code>
                    </div>
                  </div>
                </div>

                <div className="list-group-item">
                  <div className="row align-items-center">
                    <div className="col">
                      <h5 className="mb-0">Amount (SOL)</h5>
                    </div>
                    <div className="col-auto">
                      {`◎${(1.0 * transfer.lamports) / LAMPORTS_PER_SOL}`}
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        );
      })}
    </>
  );
}

export default TransactionModal;
