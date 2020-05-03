import React from "react";
import bs58 from "bs58";
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
  SystemProgram,
  SignatureResult
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

      <StatusCard signature={signature} />
      <AccountsCard signature={signature} />
      <InstructionsSection signature={signature} />
    </div>
  );
}

function StatusCard({ signature }: Props) {
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
      statusClass = "warning";
      statusText = "Error";
    }

    return (
      <h3 className="mb-0">
        <span className={`badge badge-soft-${statusClass}`}>{statusText}</span>
      </h3>
    );
  };

  const fee = details?.transaction?.meta?.fee;
  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Status</h3>
        <button className="btn btn-white btn-sm" onClick={refreshStatus}>
          <span className="fe fe-refresh-cw mr-2"></span>
          Refresh
        </button>
      </div>

      <TableCardBody>
        <tr>
          <td>Result</td>
          <td className="text-right">{renderResult()}</td>
        </tr>

        <tr>
          <td>Block</td>
          <td className="text-right">{info.slot}</td>
        </tr>

        <tr>
          <td>Confirmations</td>
          <td className="text-right text-uppercase">{info.confirmations}</td>
        </tr>

        {fee && (
          <tr>
            <td>Fee (SOL)</td>
            <td className="text-right">{lamportsToSolString(fee)}</td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}

function AccountsCard({ signature }: Props) {
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
        <h3 className="card-header-title">Accounts</h3>
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

function ixResult(result: SignatureResult, index: number) {
  if (result.err) {
    const err = result.err as any;
    const ixError = err["InstructionError"];
    if (ixError && Array.isArray(ixError)) {
      const [errorIndex, error] = ixError;
      if (Number.isInteger(errorIndex) && errorIndex === index) {
        return ["warning", `Error: ${JSON.stringify(error)}`];
      }
    }
    return ["dark"];
  }
  return ["success"];
}

type InstructionProps = {
  title: string;
  children: React.ReactNode;
  result: SignatureResult;
  index: number;
};

function InstructionCard({ title, children, result, index }: InstructionProps) {
  const [resultClass, errorString] = ixResult(result, index);
  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          <span className={`badge badge-soft-${resultClass} mr-2`}>
            #{index + 1}
          </span>
          {title}
        </h3>
        <h3 className="mb-0">
          <span className="badge badge-soft-warning text-monospace">
            {errorString}
          </span>
        </h3>
      </div>
      <TableCardBody>{children}</TableCardBody>
    </div>
  );
}

function InstructionsSection({ signature }: Props) {
  const status = useTransactionStatus(signature);
  const details = useTransactionDetails(signature);
  const dispatch = useDetailsDispatch();
  const { url } = useCluster();
  const refreshDetails = () => fetchDetails(dispatch, signature, url);

  if (!status || !status.info || !details || !details.transaction) return null;

  const { transaction } = details.transaction;
  if (transaction.instructions.length === 0) {
    return <RetryCard retry={refreshDetails} text="No instructions found" />;
  }

  const result = status.info.result;
  const instructionDetails = transaction.instructions.map((ix, index) => {
    const transfer = decodeTransfer(ix);
    if (transfer) {
      return (
        <InstructionCard
          key={index}
          title="Transfer"
          result={result}
          index={index}
        >
          <TransferDetails ix={ix} transfer={transfer} />
        </InstructionCard>
      );
    }

    const create = decodeCreate(ix);
    if (create) {
      return (
        <InstructionCard
          key={index}
          title="Create Account"
          result={result}
          index={index}
        >
          <CreateDetails ix={ix} create={create} />
        </InstructionCard>
      );
    }

    return (
      <InstructionCard key={index} title="Raw" result={result} index={index}>
        <RawDetails ix={ix} />
      </InstructionCard>
    );
  });

  return (
    <>
      <div className="container">
        <div className="header">
          <div className="header-body">
            <h3 className="mb-0">Instruction(s)</h3>
          </div>
        </div>
      </div>
      {instructionDetails}
    </>
  );
}

function TransferDetails({
  ix,
  transfer
}: {
  ix: TransactionInstruction;
  transfer: TransferParams;
}) {
  const from = transfer.fromPubkey.toBase58();
  const to = transfer.toPubkey.toBase58();
  const [fromMeta, toMeta] = ix.keys;
  return (
    <>
      <tr>
        <td>Program</td>
        <td className="text-right">
          <Copyable bottom text={SystemProgram.programId.toBase58()}>
            <code>{displayAddress(SystemProgram.programId)}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>
          <div className="mr-2 d-md-inline">From Address</div>
          {!fromMeta.isWritable && (
            <span className="badge badge-soft-dark mr-1">Readonly</span>
          )}
          {fromMeta.isSigner && (
            <span className="badge badge-soft-dark mr-1">Signer</span>
          )}
        </td>
        <td className="text-right">
          <Copyable text={from}>
            <code>{from}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>
          <div className="mr-2 d-md-inline">To Address</div>
          {!toMeta.isWritable && (
            <span className="badge badge-soft-dark mr-1">Readonly</span>
          )}
          {toMeta.isSigner && (
            <span className="badge badge-soft-dark mr-1">Signer</span>
          )}
        </td>
        <td className="text-right">
          <Copyable text={to}>
            <code>{to}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Transfer Amount (SOL)</td>
        <td className="text-right">{lamportsToSolString(transfer.lamports)}</td>
      </tr>
    </>
  );
}

function CreateDetails({
  ix,
  create
}: {
  ix: TransactionInstruction;
  create: CreateAccountParams;
}) {
  const from = create.fromPubkey.toBase58();
  const newKey = create.newAccountPubkey.toBase58();
  const [fromMeta, newMeta] = ix.keys;

  return (
    <>
      <tr>
        <td>Program</td>
        <td className="text-right">
          <Copyable bottom text={SystemProgram.programId.toBase58()}>
            <code>{displayAddress(SystemProgram.programId)}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>
          <div className="mr-2 d-md-inline">From Address</div>
          {!fromMeta.isWritable && (
            <span className="badge badge-soft-dark mr-1">Readonly</span>
          )}
          {fromMeta.isSigner && (
            <span className="badge badge-soft-dark mr-1">Signer</span>
          )}
        </td>
        <td className="text-right">
          <Copyable text={from}>
            <code>{from}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>
          <div className="mr-2 d-md-inline">New Address</div>
          {!newMeta.isWritable && (
            <span className="badge badge-soft-dark mr-1">Readonly</span>
          )}
          {newMeta.isSigner && (
            <span className="badge badge-soft-dark mr-1">Signer</span>
          )}
        </td>
        <td className="text-right">
          <Copyable text={newKey}>
            <code>{newKey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Transfer Amount (SOL)</td>
        <td className="text-right">{lamportsToSolString(create.lamports)}</td>
      </tr>

      <tr>
        <td>Allocated Space (Bytes)</td>
        <td className="text-right">{create.space}</td>
      </tr>

      <tr>
        <td>Assigned Owner</td>
        <td className="text-right">
          <Copyable text={create.programId.toBase58()}>
            <code>{displayAddress(create.programId)}</code>
          </Copyable>
        </td>
      </tr>
    </>
  );
}

function RawDetails({ ix }: { ix: TransactionInstruction }) {
  return (
    <>
      <tr>
        <td>Program</td>
        <td className="text-right">
          <Copyable bottom text={ix.programId.toBase58()}>
            <code>{displayAddress(ix.programId)}</code>
          </Copyable>
        </td>
      </tr>

      {ix.keys.map(({ pubkey, isSigner, isWritable }, keyIndex) => (
        <tr key={keyIndex}>
          <td>
            <div className="mr-2 d-md-inline">Account #{keyIndex + 1}</div>
            {!isWritable && (
              <span className="badge badge-soft-dark mr-1">Readonly</span>
            )}
            {isSigner && (
              <span className="badge badge-soft-dark mr-1">Signer</span>
            )}
          </td>
          <td className="text-right">
            <Copyable text={pubkey.toBase58()}>
              <code>{pubkey.toBase58()}</code>
            </Copyable>
          </td>
        </tr>
      ))}

      <tr>
        <td>Raw Data (Base58)</td>
        <td className="text-right">
          <Copyable text={bs58.encode(ix.data)}>
            <code>{bs58.encode(ix.data)}</code>
          </Copyable>
        </td>
      </tr>
    </>
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

function TableCardBody({ children }: { children: React.ReactNode }) {
  return (
    <div className="table-responsive mb-0">
      <table className="table table-sm table-nowrap card-table">
        <tbody className="list">{children}</tbody>
      </table>
    </div>
  );
}
