import React from "react";
import { Message, PACKET_DATA_SIZE } from "@solana/web3.js";

import { TableCardBody } from "components/common/TableCardBody";
import { SolBalance } from "utils";
import { useQuery } from "utils/url";
import { useHistory, useLocation } from "react-router";
import {
  useFetchRawTransaction,
  useRawTransactionDetails,
} from "providers/transactions/raw";
import { FetchStatus } from "providers/cache";
import { LoadingCard } from "components/common/LoadingCard";
import { ErrorCard } from "components/common/ErrorCard";
import { TransactionSignatures } from "./SignaturesCard";
import { AccountsCard } from "./AccountsCard";
import {
  AddressWithContext,
  createFeePayerValidator,
} from "./AddressWithContext";
import { SimulatorCard } from "./SimulatorCard";
import { RawInput } from "./RawInputCard";
import { InstructionsSection } from "./InstructionsSection";

export type TransactionData = {
  rawMessage: Uint8Array;
  message: Message;
  signatures?: (string | null)[];
};

export function TransactionInspectorPage({
  signature,
}: {
  signature?: string;
}) {
  const [transaction, setTransaction] = React.useState<TransactionData>();
  const query = useQuery();
  const history = useHistory();
  const location = useLocation();
  const [paramString, setParamString] = React.useState<string>();

  // Sync message with url search params
  React.useEffect(() => {
    if (signature) return;
    if (transaction) {
      const base64 = btoa(
        String.fromCharCode.apply(null, [...transaction.rawMessage])
      );
      const newParam = encodeURIComponent(base64);
      if (query.get("message") === newParam) return;
      query.set("message", newParam);
      history.push({ ...location, search: query.toString() });
    }
  }, [query, transaction, signature, history, location]);

  const reset = React.useCallback(() => {
    query.delete("message");
    history.push({ ...location, search: query.toString() });
    setTransaction(undefined);
  }, [query, location, history]);

  // Decode the message url param whenever it changes
  React.useEffect(() => {
    if (transaction || signature) return;

    let messageParam = query.get("message");
    if (messageParam !== null) {
      let messageString;
      try {
        messageString = decodeURIComponent(messageParam);
      } catch (err) {
        query.delete("message");
        history.push({ ...location, search: query.toString() });
        return;
      }

      try {
        const buffer = Uint8Array.from(atob(messageString), (c) =>
          c.charCodeAt(0)
        );

        if (buffer.length < 36) {
          query.delete("message");
          history.push({ ...location, search: query.toString() });
          throw new Error("buffer is too short");
        }

        const message = Message.from(buffer);
        setParamString(undefined);
        setTransaction({
          message,
          rawMessage: buffer,
        });
      } catch (err) {
        setParamString(messageString);
      }
    } else {
      setParamString(undefined);
    }
  }, [query, transaction, signature, history, location]);

  return (
    <div className="container mt-4">
      <div className="header">
        <div className="header-body">
          <h2 className="header-title">Transaction Inspector</h2>
        </div>
      </div>
      {signature ? (
        <PermalinkView signature={signature} reset={reset} />
      ) : transaction ? (
        <LoadedView transaction={transaction} onClear={reset} />
      ) : (
        <RawInput value={paramString} setTransactionData={setTransaction} />
      )}
    </div>
  );
}

function PermalinkView({
  signature,
}: {
  signature: string;
  reset: () => void;
}) {
  const details = useRawTransactionDetails(signature);
  const fetchTransaction = useFetchRawTransaction();
  const refreshTransaction = () => fetchTransaction(signature);
  const history = useHistory();
  const location = useLocation();
  const transaction = details?.data?.raw;
  const reset = React.useCallback(() => {
    history.push({ ...location, pathname: "/tx/inspector" });
  }, [history, location]);

  // Fetch details on load
  React.useEffect(() => {
    if (!details) fetchTransaction(signature);
  }, [signature, details, fetchTransaction]);

  if (!details || details.status === FetchStatus.Fetching) {
    return <LoadingCard />;
  } else if (details.status === FetchStatus.FetchFailed) {
    return (
      <ErrorCard
        retry={refreshTransaction}
        text="Failed to fetch transaction"
      />
    );
  } else if (!transaction) {
    return (
      <ErrorCard
        text="Transaction was not found"
        retry={reset}
        retryText="Reset"
      />
    );
  }

  const { message, signatures } = transaction;
  const tx = { message, rawMessage: message.serialize(), signatures };

  return <LoadedView transaction={tx} onClear={reset} />;
}

function LoadedView({
  transaction,
  onClear,
}: {
  transaction: TransactionData;
  onClear: () => void;
}) {
  const { message, rawMessage, signatures } = transaction;

  return (
    <>
      <OverviewCard message={message} raw={rawMessage} onClear={onClear} />
      <SimulatorCard message={message} />
      {signatures && (
        <TransactionSignatures
          message={message}
          signatures={signatures}
          rawMessage={rawMessage}
        />
      )}
      <AccountsCard message={message} />
      <InstructionsSection message={message} />
    </>
  );
}

const DEFAULT_FEES = {
  lamportsPerSignature: 5000,
};

function OverviewCard({
  message,
  raw,
  onClear,
}: {
  message: Message;
  raw: Uint8Array;
  onClear: () => void;
}) {
  const fee =
    message.header.numRequiredSignatures * DEFAULT_FEES.lamportsPerSignature;
  const feePayerValidator = createFeePayerValidator(fee);

  const size = React.useMemo(() => {
    const sigBytes = 1 + 64 * message.header.numRequiredSignatures;
    return sigBytes + raw.length;
  }, [message, raw]);

  return (
    <>
      <div className="card">
        <div className="card-header">
          <h3 className="card-header-title">Transaction Overview</h3>
          <button className="btn btn-sm d-flex btn-white" onClick={onClear}>
            Clear
          </button>
        </div>
        <TableCardBody>
          <tr>
            <td>Serialized Size</td>
            <td className="text-lg-right">
              <div className="d-flex align-items-end flex-column">
                {size} bytes
                <span
                  className={
                    size <= PACKET_DATA_SIZE ? "text-muted" : "text-warning"
                  }
                >
                  Max transaction size is {PACKET_DATA_SIZE} bytes
                </span>
              </div>
            </td>
          </tr>
          <tr>
            <td>Fees</td>
            <td className="text-lg-right">
              <div className="d-flex align-items-end flex-column">
                <SolBalance lamports={fee} />
                <span className="text-muted">
                  {`Each signature costs ${DEFAULT_FEES.lamportsPerSignature} lamports`}
                </span>
              </div>
            </td>
          </tr>
          <tr>
            <td>
              <div className="d-flex align-items-start flex-column">
                Fee payer
                <span className="mt-1">
                  <span className="badge badge-soft-info mr-2">Signer</span>
                  <span className="badge badge-soft-danger mr-2">Writable</span>
                </span>
              </div>
            </td>
            <td className="text-right">
              {message.accountKeys.length === 0 ? (
                "No Fee Payer"
              ) : (
                <AddressWithContext
                  pubkey={message.accountKeys[0]}
                  validator={feePayerValidator}
                />
              )}
            </td>
          </tr>
        </TableCardBody>
      </div>
    </>
  );
}
