import React from "react";
import { PACKET_DATA_SIZE, VersionedMessage } from "@solana/web3.js";

import { TableCardBody } from "components/common/TableCardBody";
import { SolBalance } from "components/common/SolBalance";
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
import { AddressTableLookupsCard } from "./AddressTableLookupsCard";
import {
  AddressWithContext,
  createFeePayerValidator,
} from "./AddressWithContext";
import { SimulatorCard } from "./SimulatorCard";
import { MIN_MESSAGE_LENGTH, RawInput } from "./RawInputCard";
import { InstructionsSection } from "./InstructionsSection";
import base58 from "bs58";
import { useFetchAccountInfo } from "providers/accounts";

export type TransactionData = {
  rawMessage: Uint8Array;
  message: VersionedMessage;
  signatures?: (string | null)[];
};

// Decode a url param and return the result. If decoding fails, return whether
// the param should be deleted.
function decodeParam(params: URLSearchParams, name: string): string | boolean {
  const param = params.get(name);
  if (param === null) return false;
  try {
    return decodeURIComponent(param);
  } catch (err) {
    return true;
  }
}

// Decode a signatures param and throw an error on failure
function decodeSignatures(signaturesParam: string): (string | null)[] {
  let signatures;
  try {
    signatures = JSON.parse(signaturesParam);
  } catch (err) {
    throw new Error("Signatures param is not valid JSON");
  }

  if (!Array.isArray(signatures)) {
    throw new Error("Signatures param is not a JSON array");
  }

  const validSignatures: (string | null)[] = [];
  for (const signature of signatures) {
    if (signature === null) {
      validSignatures.push(signature);
      continue;
    }

    if (typeof signature !== "string") {
      throw new Error("Signature is not a string");
    }

    try {
      base58.decode(signature);
      validSignatures.push(signature);
    } catch (err) {
      throw new Error("Signature is not valid base58");
    }
  }

  return validSignatures;
}

// Decodes url params into transaction data if possible. If decoding fails,
// URL params are returned as a string that will prefill the transaction
// message input field for debugging. Returns a tuple of [result, shouldRefreshUrl]
function decodeUrlParams(
  params: URLSearchParams
): [TransactionData | string, boolean] {
  const messageParam = decodeParam(params, "message");
  const signaturesParam = decodeParam(params, "signatures");

  let refreshUrl = false;
  if (signaturesParam === true) {
    params.delete("signatures");
    refreshUrl = true;
  }

  if (typeof messageParam === "boolean") {
    if (messageParam) {
      params.delete("message");
      params.delete("signatures");
      refreshUrl = true;
    }
    return ["", refreshUrl];
  }

  let signatures: (string | null)[] | undefined = undefined;
  if (typeof signaturesParam === "string") {
    try {
      signatures = decodeSignatures(signaturesParam);
    } catch (err) {
      params.delete("signatures");
      refreshUrl = true;
    }
  }

  try {
    const buffer = Uint8Array.from(atob(messageParam), (c) => c.charCodeAt(0));

    if (buffer.length < MIN_MESSAGE_LENGTH) {
      throw new Error("message buffer is too short");
    }

    const message = VersionedMessage.deserialize(buffer);
    const data = {
      message,
      rawMessage: buffer,
      signatures,
    };
    return [data, refreshUrl];
  } catch (err) {
    params.delete("message");
    refreshUrl = true;
    return [messageParam, true];
  }
}

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
      let shouldRefreshUrl = false;

      if (transaction.signatures !== undefined) {
        const signaturesParam = encodeURIComponent(
          JSON.stringify(transaction.signatures)
        );
        if (query.get("signatures") !== signaturesParam) {
          shouldRefreshUrl = true;
          query.set("signatures", signaturesParam);
        }
      }

      const base64 = btoa(
        String.fromCharCode.apply(null, [...transaction.rawMessage])
      );
      const newParam = encodeURIComponent(base64);
      if (query.get("message") !== newParam) {
        shouldRefreshUrl = true;
        query.set("message", newParam);
      }

      if (shouldRefreshUrl) {
        history.push({ ...location, search: query.toString() });
      }
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

    const [result, refreshUrl] = decodeUrlParams(query);
    if (refreshUrl) {
      history.push({ ...location, search: query.toString() });
    }

    if (typeof result === "string") {
      setParamString(result);
    } else {
      setTransaction(result);
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

  const fetchAccountInfo = useFetchAccountInfo();
  React.useEffect(() => {
    for (let lookup of message.addressTableLookups) {
      fetchAccountInfo(lookup.accountKey, "parsed");
    }
  }, [message, fetchAccountInfo]);

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
      <AddressTableLookupsCard message={message} />
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
  message: VersionedMessage;
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
            <td className="text-lg-end">
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
            <td className="text-lg-end">
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
                  <span className="badge bg-info-soft me-2">Signer</span>
                  <span className="badge bg-danger-soft me-2">Writable</span>
                </span>
              </div>
            </td>
            <td className="text-end">
              {message.staticAccountKeys.length === 0 ? (
                "No Fee Payer"
              ) : (
                <AddressWithContext
                  pubkey={message.staticAccountKeys[0]}
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
