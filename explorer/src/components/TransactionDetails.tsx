import React from "react";
import {
  Source,
  useTransactionStatus,
  useTransactionDetails,
  useTransactionsDispatch,
  useDetailsDispatch,
  checkTransactionStatus,
  ActionType,
  FetchStatus
} from "../providers/transactions";
import { fetchDetails } from "providers/transactions/details";
import { useCluster, useClusterModal } from "providers/cluster";
import {
  TransactionSignature,
  TransactionInstruction,
  TransferParams,
  CreateAccountParams,
  SystemProgram
} from "@solana/web3.js";
import ClusterStatusButton from "components/ClusterStatusButton";
import { lamportsToSolString } from "utils";
import { displayAddress, decodeCreate, decodeTransfer } from "utils/tx";
import Copyable from "./Copyable";
import { useHistory, useLocation } from "react-router-dom";

type Props = { signature: TransactionSignature };
export default function TransactionDetails({ signature }: Props) {
  const dispatch = useTransactionsDispatch();
  const { url } = useCluster();
  const [, setShow] = useClusterModal();
  const [search, setSearch] = React.useState(signature);
  const history = useHistory();
  const location = useLocation();

  const updateSignature = () => {
    history.push({ ...location, pathname: "/tx/" + search });
  };

  // Fetch transaction on load
  React.useEffect(() => {
    dispatch({
      type: ActionType.FetchSignature,
      signature,
      source: Source.Url
    });
    checkTransactionStatus(dispatch, signature, url);
  }, [signature, dispatch, url]);

  const searchInput = (
    <input
      type="text"
      value={search}
      onChange={e => setSearch(e.target.value)}
      onKeyUp={e => e.key === "Enter" && updateSignature()}
      className="form-control form-control-prepended search text-monospace"
      placeholder="Search for signature"
    />
  );

  return (
    <div className="container">
      <div className="header">
        <div className="header-body">
          <div className="row align-items-center">
            <div className="col">
              <h6 className="header-pretitle">Details</h6>
              <h3 className="header-title">Transaction</h3>
            </div>
            <div className="col-auto">
              <ClusterStatusButton onClick={() => setShow(true)} />
            </div>
          </div>
        </div>
      </div>

      <div className="row mb-4 mt-n2 align-items-center">
        <div className="col d-none d-md-block">
          <div className="input-group input-group-merge">
            {searchInput}
            <div className="input-group-prepend">
              <div className="input-group-text">
                <span className="fe fe-search"></span>
              </div>
            </div>
          </div>
        </div>
        <div className="col d-block d-md-none">{searchInput}</div>
        <div className="col-auto ml-n3 d-block d-md-none">
          <button className="btn btn-white" onClick={updateSignature}>
            <span className="fe fe-search"></span>
          </button>
        </div>
      </div>

      <TransactionStatusCard signature={signature} />
      <TransactionAccountsCard signature={signature} />
      <TransactionInstructionsCard signature={signature} />
    </div>
  );
}

function TransactionStatusCard({ signature }: Props) {
  const status = useTransactionStatus(signature);
  const dispatch = useTransactionsDispatch();
  const details = useTransactionDetails(signature);
  const { url } = useCluster();

  const refreshStatus = () => {
    checkTransactionStatus(dispatch, signature, url);
  };

  if (!status || status.fetchStatus === FetchStatus.Fetching) {
    return <LoadingCard />;
  } else if (status?.fetchStatus === FetchStatus.FetchFailed) {
    return <RetryCard retry={refreshStatus} text="Fetch Failed" />;
  } else if (!status.info) {
    return <RetryCard retry={refreshStatus} text="Not Found" />;
  }

  const { info } = status;
  const renderResult = () => {
    let statusClass = "success";
    let statusText = "Success";
    if (info.result.err) {
      statusClass = "danger";
      statusText = "Error";
    }

    return (
      <span className={`badge badge-soft-${statusClass}`}>{statusText}</span>
    );
  };

  const fee = details?.transaction?.meta?.fee;
  return (
    <div className="card">
      <div className="card-header">
        <div className="row align-items-center">
          <div className="col">
            <div className="card-header-title">Status</div>
          </div>
          <div className="col-auto">
            <button className="btn btn-white btn-sm" onClick={refreshStatus}>
              <span className="fe fe-refresh-cw mr-2"></span>
              Refresh
            </button>
          </div>
        </div>
      </div>
      <div className="card-body">
        <div className="list-group list-group-flush my-n3">
          <div className="list-group-item">
            <div className="row align-items-center">
              <div className="col">
                <h5 className="mb-0">Result</h5>
              </div>
              <div className="col-auto">{renderResult()}</div>
            </div>
          </div>

          <div className="list-group-item">
            <div className="row align-items-center">
              <div className="col">
                <h5 className="mb-0">Block</h5>
              </div>
              <div className="col-auto">{info.slot}</div>
            </div>
          </div>

          <div className="list-group-item">
            <div className="row align-items-center">
              <div className="col">
                <h5 className="mb-0">Confirmations</h5>
              </div>
              <div className="col-auto text-uppercase">
                {info.confirmations}
              </div>
            </div>
          </div>
          {fee && (
            <div className="list-group-item">
              <div className="row align-items-center">
                <div className="col">
                  <h5 className="mb-0">Fee (SOL)</h5>
                </div>
                <div className="col-auto">{lamportsToSolString(fee)}</div>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function TransactionAccountsCard({ signature }: Props) {
  const details = useTransactionDetails(signature);
  const dispatch = useDetailsDispatch();
  const { url } = useCluster();

  const refreshDetails = () => fetchDetails(dispatch, signature, url);
  const transaction = details?.transaction?.transaction;
  const message = React.useMemo(() => {
    return transaction?.compileMessage();
  }, [transaction]);

  if (!details) {
    return null;
  } else if (details.fetchStatus === FetchStatus.Fetching) {
    return <LoadingCard />;
  } else if (details?.fetchStatus === FetchStatus.FetchFailed) {
    return <RetryCard retry={refreshDetails} text="Fetch Failed" />;
  } else if (!details.transaction || !message) {
    return <RetryCard retry={refreshDetails} text="Not Found" />;
  }

  const { meta } = details.transaction;
  if (!meta) {
    return <RetryCard retry={refreshDetails} text="Metadata Missing" />;
  }

  const accountRows = message.accountKeys.map((pubkey, index) => {
    const pre = meta.preBalances[index];
    const post = meta.postBalances[index];
    const key = pubkey.toBase58();
    const renderChange = () => {
      const change = post - pre;
      if (change === 0) return "";
      const sols = lamportsToSolString(change);
      if (change > 0) {
        return <span className="badge badge-soft-success">{"+" + sols}</span>;
      } else {
        return <span className="badge badge-soft-warning">{"-" + sols}</span>;
      }
    };

    return (
      <tr key={key}>
        <td>
          <Copyable text={key}>
            <code>{displayAddress(pubkey)}</code>
          </Copyable>
        </td>
        <td>{renderChange()}</td>
        <td>{lamportsToSolString(post)}</td>
        <td>
          {index === 0 && (
            <span className="badge badge-soft-dark mr-1">Fee Payer</span>
          )}
          {!message.isAccountWritable(index) && (
            <span className="badge badge-soft-dark mr-1">Readonly</span>
          )}
          {index < message.header.numRequiredSignatures && (
            <span className="badge badge-soft-dark mr-1">Signer</span>
          )}
          {message.instructions.find(ix => ix.programIdIndex === index) && (
            <span className="badge badge-soft-dark mr-1">Program</span>
          )}
        </td>
      </tr>
    );
  });

  return (
    <div className="card">
      <div className="card-header">
        <h4 className="card-header-title">Accounts</h4>
      </div>
      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">Address</th>
              <th className="text-muted">Change (SOL)</th>
              <th className="text-muted">Post Balance (SOL)</th>
              <th className="text-muted">Details</th>
            </tr>
          </thead>
          <tbody className="list">{accountRows}</tbody>
        </table>
      </div>
    </div>
  );
}

function TransactionInstructionsCard({ signature }: Props) {
  const details = useTransactionDetails(signature);
  const dispatch = useDetailsDispatch();
  const { url } = useCluster();
  const refreshDetails = () => fetchDetails(dispatch, signature, url);

  if (!details || !details.transaction) return null;

  const { transaction } = details.transaction;
  if (transaction.instructions.length === 0) {
    return <RetryCard retry={refreshDetails} text="No instructions found" />;
  }

  const instructionDetails = transaction.instructions.map((ix, index) => {
    const transfer = decodeTransfer(ix);
    if (transfer)
      return <TransferDetails key={index} transfer={transfer} index={index} />;
    const create = decodeCreate(ix);
    if (create)
      return <CreateDetails key={index} create={create} index={index} />;
    return <InstructionDetails key={index} ix={ix} index={index} />;
  });

  return (
    <>
      <div className="container">
        <div className="header">
          <div className="header-body">Transaction Instruction(s)</div>
        </div>
      </div>
      {instructionDetails}
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
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          <span className="badge badge-soft-dark mr-2">#{index + 1}</span>
          Transfer
        </h3>
      </div>
      <div className="card-body">
        <div className="list-group list-group-flush my-n3">
          <ListGroupItem label="Program">
            <code>{displayAddress(SystemProgram.programId)}</code>
          </ListGroupItem>
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
            {lamportsToSolString(transfer.lamports)}
          </ListGroupItem>
        </div>
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
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          <span className="badge badge-soft-dark mr-2">#{index + 1}</span>
          Create Account
        </h3>
      </div>
      <div className="card-body">
        <div className="list-group list-group-flush my-n3">
          <ListGroupItem label="Program">
            <code>{displayAddress(SystemProgram.programId)}</code>
          </ListGroupItem>
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
            {lamportsToSolString(create.lamports)}
          </ListGroupItem>
          <ListGroupItem label="Data (Bytes)">{create.space}</ListGroupItem>
          <ListGroupItem label="Owner">
            <code>{displayAddress(create.programId)}</code>
          </ListGroupItem>
        </div>
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
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          <span className="badge badge-soft-dark mr-2">#{index + 1}</span>
        </h3>
      </div>
      <div className="card-body">
        <div className="list-group list-group-flush my-n3">
          <ListGroupItem label="Program">
            <code>{displayAddress(ix.programId)}</code>
          </ListGroupItem>
          {ix.keys.map(({ pubkey }, keyIndex) => (
            <ListGroupItem key={keyIndex} label={`Address #${keyIndex + 1}`}>
              <Copyable text={pubkey.toBase58()}>
                <code>{pubkey.toBase58()}</code>
              </Copyable>
            </ListGroupItem>
          ))}
          <ListGroupItem label="Data (Bytes)">{ix.data.length}</ListGroupItem>
        </div>
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

function LoadingCard() {
  return (
    <div className="card">
      <div className="card-body">
        <span className="spinner-grow spinner-grow-sm mr-2"></span>
        Loading
      </div>
    </div>
  );
}

function RetryCard({ retry, text }: { retry: () => void; text: string }) {
  return (
    <div className="card">
      <div className="card-body">
        {text}
        <span className="btn btn-white ml-3" onClick={retry}>
          Try Again
        </span>
      </div>
    </div>
  );
}
