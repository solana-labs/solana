import React from "react";
import {
  useTransactions,
  useTransactionsDispatch,
  ActionType,
  Selected
} from "../providers/transactions";
import { displayAddress } from "../utils";
import { useBlocks } from "../providers/blocks";
import {
  LAMPORTS_PER_SOL,
  TransferParams,
  CreateAccountParams
} from "@solana/web3.js";

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

  const renderError = (content: React.ReactNode) => {
    return (
      <div className="card-body">
        <span className="text-info">{content}</span>
      </div>
    );
  };

  if (!block) return renderError("Transaction block not found");

  if (!block.transactions) {
    return renderError(
      <>
        <span className="spinner-grow spinner-grow-sm mr-2"></span>
        Loading
      </>
    );
  }

  const details = block.transactions[selected.signature];
  if (!details) return renderError("Transaction not found");

  const { transfers, creates } = details;
  if (transfers.length === 0 && creates.length === 0)
    return renderError(
      "Details for this transaction's instructions are not yet supported"
    );

  let i = 0;
  return (
    <>
      {details.transfers.map(transfer => {
        return (
          <div key={++i}>
            {i > 1 ? <hr className="mb-4"></hr> : null}
            <TransferDetails transfer={transfer} />
          </div>
        );
      })}
      {details.creates.map(create => {
        return (
          <div key={++i}>
            {i > 1 ? <hr className="mb-4"></hr> : null}
            <CreateDetails create={create} />
          </div>
        );
      })}
    </>
  );
}

function TransferDetails({ transfer }: { transfer: TransferParams }) {
  return (
    <div className="card-body">
      <div className="list-group list-group-flush my-n3">
        <ListGroupItem label="From">
          <code>{transfer.fromPubkey.toBase58()}</code>
        </ListGroupItem>
        <ListGroupItem label="To">
          <code>{transfer.toPubkey.toBase58()}</code>
        </ListGroupItem>
        <ListGroupItem label="Amount (SOL)">
          {`◎${(1.0 * transfer.lamports) / LAMPORTS_PER_SOL}`}
        </ListGroupItem>
      </div>
    </div>
  );
}

function CreateDetails({ create }: { create: CreateAccountParams }) {
  return (
    <div className="card-body">
      <div className="list-group list-group-flush my-n3">
        <ListGroupItem label="From">
          <code>{create.fromPubkey.toBase58()}</code>
        </ListGroupItem>
        <ListGroupItem label="New Account">
          <code>{create.newAccountPubkey.toBase58()}</code>
        </ListGroupItem>
        <ListGroupItem label="Amount (SOL)">
          {`◎${(1.0 * create.lamports) / LAMPORTS_PER_SOL}`}
        </ListGroupItem>
        <ListGroupItem label="Data (Bytes)">{create.space}</ListGroupItem>
        <ListGroupItem label="Owner">
          <code>{displayAddress(create.programId)}</code>
        </ListGroupItem>
      </div>
    </div>
  );
}

function ListGroupItem({
  label,
  children
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="list-group-item">
      <div className="row align-items-center">
        <div className="col">
          <h5 className="mb-0">{label}</h5>
        </div>
        <div className="col-auto">{children}</div>
      </div>
    </div>
  );
}

export default TransactionModal;
