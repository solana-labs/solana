import React from "react";
import { Link } from "react-router-dom";
import bs58 from "bs58";
import {
  useFetchTransactionStatus,
  useTransactionStatus,
  useTransactionDetails,
} from "providers/transactions";
import { useFetchTransactionDetails } from "providers/transactions/parsed";
import { useCluster, ClusterStatus } from "providers/cluster";
import {
  TransactionSignature,
  SystemProgram,
  SystemInstruction,
} from "@solana/web3.js";
import { SolBalance } from "utils";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { TableCardBody } from "components/common/TableCardBody";
import { displayTimestamp } from "utils/date";
import { InfoTooltip } from "components/common/InfoTooltip";
import { Address } from "components/common/Address";
import { Signature } from "components/common/Signature";
import { intoTransactionInstruction } from "utils/tx";
import { FetchStatus } from "providers/cache";
import { Slot } from "components/common/Slot";
import { BigNumber } from "bignumber.js";
import { BalanceDelta } from "components/common/BalanceDelta";
import { TokenBalancesCard } from "components/transaction/TokenBalancesCard";
import { InstructionsSection } from "components/transaction/InstructionsSection";
import { ProgramLogSection } from "components/transaction/ProgramLogSection";
import { clusterPath } from "utils/url";
import { getTransactionInstructionError } from "utils/program-err";

const AUTO_REFRESH_INTERVAL = 2000;
const ZERO_CONFIRMATION_BAILOUT = 5;
export const INNER_INSTRUCTIONS_START_SLOT = 46915769;

export type SignatureProps = {
  signature: TransactionSignature;
};

export const SignatureContext = React.createContext("");

enum AutoRefresh {
  Active,
  Inactive,
  BailedOut,
}

type AutoRefreshProps = {
  autoRefresh: AutoRefresh;
};

export function TransactionDetailsPage({ signature: raw }: SignatureProps) {
  let signature: TransactionSignature | undefined;

  try {
    const decoded = bs58.decode(raw);
    if (decoded.length === 64) {
      signature = raw;
    }
  } catch (err) {}

  const status = useTransactionStatus(signature);
  const [zeroConfirmationRetries, setZeroConfirmationRetries] =
    React.useState(0);

  let autoRefresh = AutoRefresh.Inactive;

  if (zeroConfirmationRetries >= ZERO_CONFIRMATION_BAILOUT) {
    autoRefresh = AutoRefresh.BailedOut;
  } else if (status?.data?.info && status.data.info.confirmations !== "max") {
    autoRefresh = AutoRefresh.Active;
  }

  React.useEffect(() => {
    if (
      status?.status === FetchStatus.Fetched &&
      status.data?.info &&
      status.data.info.confirmations === 0
    ) {
      setZeroConfirmationRetries((retries) => retries + 1);
    }
  }, [status]);

  React.useEffect(() => {
    if (
      status?.status === FetchStatus.Fetching &&
      autoRefresh === AutoRefresh.BailedOut
    ) {
      setZeroConfirmationRetries(0);
    }
  }, [status, autoRefresh, setZeroConfirmationRetries]);

  return (
    <div className="container mt-n3">
      <div className="header">
        <div className="header-body">
          <h6 className="header-pretitle">Details</h6>
          <h2 className="header-title">Transaction</h2>
        </div>
      </div>
      {signature === undefined ? (
        <ErrorCard text={`Signature "${raw}" is not valid`} />
      ) : (
        <SignatureContext.Provider value={signature}>
          <StatusCard signature={signature} autoRefresh={autoRefresh} />
          <React.Suspense
            fallback={<LoadingCard message="Loading transaction details" />}
          >
            <DetailsSection signature={signature} />
          </React.Suspense>
        </SignatureContext.Provider>
      )}
    </div>
  );
}

function StatusCard({
  signature,
  autoRefresh,
}: SignatureProps & AutoRefreshProps) {
  const fetchStatus = useFetchTransactionStatus();
  const status = useTransactionStatus(signature);
  const details = useTransactionDetails(signature);
  const { clusterInfo, status: clusterStatus } = useCluster();

  // Fetch transaction on load
  React.useEffect(() => {
    if (!status && clusterStatus === ClusterStatus.Connected) {
      fetchStatus(signature);
    }
  }, [signature, clusterStatus]); // eslint-disable-line react-hooks/exhaustive-deps

  // Effect to set and clear interval for auto-refresh
  React.useEffect(() => {
    if (autoRefresh === AutoRefresh.Active) {
      let intervalHandle: NodeJS.Timeout = setInterval(
        () => fetchStatus(signature),
        AUTO_REFRESH_INTERVAL
      );

      return () => {
        clearInterval(intervalHandle);
      };
    }
  }, [autoRefresh, fetchStatus, signature]);

  if (
    !status ||
    (status.status === FetchStatus.Fetching &&
      autoRefresh === AutoRefresh.Inactive)
  ) {
    return <LoadingCard />;
  } else if (status.status === FetchStatus.FetchFailed) {
    return (
      <ErrorCard retry={() => fetchStatus(signature)} text="Fetch Failed" />
    );
  } else if (!status.data?.info) {
    if (clusterInfo && clusterInfo.firstAvailableBlock > 0) {
      return (
        <ErrorCard
          retry={() => fetchStatus(signature)}
          text="Not Found"
          subtext={`Note: Transactions processed before block ${clusterInfo.firstAvailableBlock} are not available at this time`}
        />
      );
    }
    return <ErrorCard retry={() => fetchStatus(signature)} text="Not Found" />;
  }

  const { info } = status.data;

  let statusClass = "success";
  let statusText = "Success";
  let errorReason = undefined;

  if (info.result.err) {
    statusClass = "warning";
    statusText = "Error";
    if (typeof info.result.err === "string") {
      errorReason = `Runtime Error: "${info.result.err}"`;
    } else {
      const programError = getTransactionInstructionError(info.result.err);
      if (programError !== undefined) {
        errorReason = `Program Error: "Instruction #${
          programError.index + 1
        } Failed"`;
      } else {
        errorReason = `Unknown Error: "${JSON.stringify(info.result.err)}"`;
      }
    }
  }

  const fee = details?.data?.transaction?.meta?.fee;
  const transaction = details?.data?.transaction?.transaction;
  const blockhash = transaction?.message.recentBlockhash;
  const isNonce = (() => {
    if (!transaction || transaction.message.instructions.length < 1) {
      return false;
    }

    const ix = intoTransactionInstruction(
      transaction,
      transaction.message.instructions[0]
    );
    return (
      ix &&
      SystemProgram.programId.equals(ix.programId) &&
      SystemInstruction.decodeInstructionType(ix) === "AdvanceNonceAccount"
    );
  })();

  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Overview</h3>
        <Link
          to={clusterPath(`/tx/${signature}/inspect`)}
          className="btn btn-white btn-sm me-2"
        >
          <span className="fe fe-settings me-2"></span>
          Inspect
        </Link>
        {autoRefresh === AutoRefresh.Active ? (
          <span className="spinner-grow spinner-grow-sm"></span>
        ) : (
          <button
            className="btn btn-white btn-sm"
            onClick={() => fetchStatus(signature)}
          >
            <span className="fe fe-refresh-cw me-2"></span>
            Refresh
          </button>
        )}
      </div>

      <TableCardBody>
        <tr>
          <td>Signature</td>
          <td className="text-lg-end">
            <Signature signature={signature} alignRight />
          </td>
        </tr>

        <tr>
          <td>Result</td>
          <td className="text-lg-end">
            <h3 className="mb-0">
              <span className={`badge bg-${statusClass}-soft`}>
                {statusText}
              </span>
            </h3>
          </td>
        </tr>

        {errorReason !== undefined && (
          <tr>
            <td>Error</td>
            <td className="text-lg-end">
              <h3 className="mb-0">
                <span className={`badge bg-${statusClass}-soft`}>
                  {errorReason}
                </span>
              </h3>
            </td>
          </tr>
        )}

        <tr>
          <td>Timestamp</td>
          <td className="text-lg-end">
            {info.timestamp !== "unavailable" ? (
              <span className="font-monospace">
                {displayTimestamp(info.timestamp * 1000)}
              </span>
            ) : (
              <InfoTooltip
                bottom
                right
                text="Timestamps are only available for confirmed blocks"
              >
                Unavailable
              </InfoTooltip>
            )}
          </td>
        </tr>

        <tr>
          <td>Confirmation Status</td>
          <td className="text-lg-end text-uppercase">
            {info.confirmationStatus || "Unknown"}
          </td>
        </tr>

        <tr>
          <td>Confirmations</td>
          <td className="text-lg-end text-uppercase">{info.confirmations}</td>
        </tr>

        <tr>
          <td>Slot</td>
          <td className="text-lg-end">
            <Slot slot={info.slot} link />
          </td>
        </tr>

        {blockhash && (
          <tr>
            <td>
              {isNonce ? (
                "Nonce"
              ) : (
                <InfoTooltip text="Transactions use a previously confirmed blockhash as a nonce to prevent double spends">
                  Recent Blockhash
                </InfoTooltip>
              )}
            </td>
            <td className="text-lg-end">{blockhash}</td>
          </tr>
        )}

        {fee && (
          <tr>
            <td>Fee (SOL)</td>
            <td className="text-lg-end">
              <SolBalance lamports={fee} />
            </td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}

function DetailsSection({ signature }: SignatureProps) {
  const details = useTransactionDetails(signature);
  const fetchDetails = useFetchTransactionDetails();
  const status = useTransactionStatus(signature);
  const transaction = details?.data?.transaction?.transaction;
  const message = transaction?.message;
  const { status: clusterStatus } = useCluster();
  const refreshDetails = () => fetchDetails(signature);

  // Fetch details on load
  React.useEffect(() => {
    if (
      !details &&
      clusterStatus === ClusterStatus.Connected &&
      status?.status === FetchStatus.Fetched
    ) {
      fetchDetails(signature);
    }
  }, [signature, clusterStatus, status]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!status?.data?.info) {
    return null;
  } else if (!details || details.status === FetchStatus.Fetching) {
    return <LoadingCard />;
  } else if (details.status === FetchStatus.FetchFailed) {
    return <ErrorCard retry={refreshDetails} text="Failed to fetch details" />;
  } else if (!details.data?.transaction || !message) {
    return <ErrorCard text="Details are not available" />;
  }

  return (
    <>
      <AccountsCard signature={signature} />
      <TokenBalancesCard signature={signature} />
      <InstructionsSection signature={signature} />
      <ProgramLogSection signature={signature} />
    </>
  );
}

function AccountsCard({ signature }: SignatureProps) {
  const details = useTransactionDetails(signature);

  if (!details?.data?.transaction) {
    return null;
  }

  const { meta, transaction } = details.data.transaction;
  const { message } = transaction;

  if (!meta) {
    return <ErrorCard text="Transaction metadata is missing" />;
  }

  const accountRows = message.accountKeys.map((account, index) => {
    const pre = meta.preBalances[index];
    const post = meta.postBalances[index];
    const pubkey = account.pubkey;
    const key = account.pubkey.toBase58();
    const delta = new BigNumber(post).minus(new BigNumber(pre));

    return (
      <tr key={key}>
        <td>{index + 1}</td>
        <td>
          <Address pubkey={pubkey} link />
        </td>
        <td>
          <BalanceDelta delta={delta} isSol />
        </td>
        <td>
          <SolBalance lamports={post} />
        </td>
        <td>
          {index === 0 && (
            <span className="badge bg-info-soft me-1">Fee Payer</span>
          )}
          {account.writable && (
            <span className="badge bg-info-soft me-1">Writable</span>
          )}
          {account.signer && (
            <span className="badge bg-info-soft me-1">Signer</span>
          )}
          {message.instructions.find((ix) => ix.programId.equals(pubkey)) && (
            <span className="badge bg-info-soft me-1">Program</span>
          )}
        </td>
      </tr>
    );
  });

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title">Account Input(s)</h3>
      </div>
      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">#</th>
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
