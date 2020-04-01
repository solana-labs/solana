import React from "react";
import {
  useTransactions,
  useTransactionsDispatch,
  ActionType,
  Selected
} from "../providers/transactions";
import { useBlocks } from "../providers/blocks";

function TransactionModal() {
  const { selected } = useTransactions();
  const dispatch = useTransactionsDispatch();
  const onClose = () => dispatch({ type: ActionType.Deselect });
  const show = !!selected;

  const renderContent = () => {
    if (!selected) return null;
    return (
      <div className="modal-dialog modal-dialog-center">
        <div className="modal-content">
          <div className="modal-body" onClick={e => e.stopPropagation()}>
            <span className="close" onClick={onClose}>
              &times;
            </span>

            <h2 className="text-center mb-4 mt-4">Transaction Details</h2>

            <TransactionDetails selected={selected} />
          </div>
        </div>
      </div>
    );
  };

  return (
    <div
      className={`modal fade fixed-right${show ? " show" : ""}`}
      onClick={onClose}
    >
      {renderContent()}
    </div>
  );
}

function TransactionDetails({ selected }: { selected: Selected }) {
  const { blocks } = useBlocks();
  const block = blocks[selected.slot];
  if (!block) return <span>{"block not found"}</span>;

  if (!block.transactions) {
    return <span>loading</span>;
  }

  const tx = block.transactions[selected.signature];
  if (!tx) return <span>{"sig not found"}</span>;

  return <code>{JSON.stringify(tx)}</code>;
}

export default TransactionModal;
