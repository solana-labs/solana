import React from "react";
import {
  useTransactions,
  useTransactionsDispatch,
  ActionType,
  Selected
} from "../providers/transactions";
import { displayAddress, decodeCreate, decodeTransfer } from "../utils/tx";
import { useBlocks } from "../providers/blocks";
import {
  LAMPORTS_PER_SOL,
  TransferParams,
  CreateAccountParams,
  TransactionInstruction
} from "@solana/web3.js";
import Copyable from "./Copyable";

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

  const transaction = block.transactions[selected.signature];
  if (!transaction) return renderError("Transaction not found");

  if (transaction.instructions.length === 0)
    return renderError("No instructions found");

  const instructionDetails = transaction.instructions.map((ix, index) => {
    const transfer = decodeTransfer(ix);
    if (transfer) return <TransferDetails transfer={transfer} index={index} />;
    const create = decodeCreate(ix);
    if (create) return <CreateDetails create={create} index={index} />;
    return <InstructionDetails ix={ix} index={index} />;
  });

  return (
    <>
      {instructionDetails.map((details, i) => {
        return (
          <div key={++i}>
            {i > 1 ? <hr className="mt-0 mb-0"></hr> : null}
            {details}
          </div>
        );
      })}
    </>
  );
}

function TransferDetails({
  transfer,
  index
}: {
  transfer: TransferParams;
  index: number;
}) {
  const from = transfer.fromPubkey.toBase58();
  const to = transfer.toPubkey.toBase58();
  return (
    <div className="card-body">
      <h4 className="ix-pill">{`Instruction #${index + 1} (Transfer)`}</h4>
      <div className="list-group list-group-flush my-n3">
        <ListGroupItem label="From">
          <Copyable text={from}>
            <code>{from}</code>
          </Copyable>
        </ListGroupItem>
        <ListGroupItem label="To">
          <Copyable text={to}>
            <code>{to}</code>
          </Copyable>
        </ListGroupItem>
        <ListGroupItem label="Amount (SOL)">
          {`◎${(1.0 * transfer.lamports) / LAMPORTS_PER_SOL}`}
        </ListGroupItem>
      </div>
    </div>
  );
}

function CreateDetails({
  create,
  index
}: {
  create: CreateAccountParams;
  index: number;
}) {
  const from = create.fromPubkey.toBase58();
  const newKey = create.newAccountPubkey.toBase58();
  return (
    <div className="card-body">
      <h4 className="ix-pill">{`Instruction #${index +
        1} (Create Account)`}</h4>
      <div className="list-group list-group-flush my-n3">
        <ListGroupItem label="From">
          <Copyable text={from}>
            <code>{from}</code>
          </Copyable>
        </ListGroupItem>
        <ListGroupItem label="New Account">
          <Copyable text={newKey}>
            <code>{newKey}</code>
          </Copyable>
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

function InstructionDetails({
  ix,
  index
}: {
  ix: TransactionInstruction;
  index: number;
}) {
  return (
    <div className="card-body">
      <h4 className="ix-pill">{`Instruction #${index + 1}`}</h4>
      <div className="list-group list-group-flush my-n3">
        {ix.keys.map(({ pubkey }, keyIndex) => (
          <ListGroupItem key={keyIndex} label={`Address #${keyIndex + 1}`}>
            <Copyable text={pubkey.toBase58()}>
              <code>{pubkey.toBase58()}</code>
            </Copyable>
          </ListGroupItem>
        ))}
        <ListGroupItem label="Data (Bytes)">{ix.data.length}</ListGroupItem>
        <ListGroupItem label="Program">
          <code>{displayAddress(ix.programId)}</code>
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
    <div className="list-group-item ix-item">
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
