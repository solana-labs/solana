import React from "react";
import bs58 from "bs58";
import {
  useFetchTransactionStatus,
  useTransactionStatus,
  useTransactionDetails,
} from "providers/transactions";
import { useFetchTransactionDetails } from "providers/transactions/details";
import { useCluster, ClusterStatus } from "providers/cluster";
import {
  TransactionSignature,
  SystemProgram,
  StakeProgram,
  SystemInstruction,
} from "@solana/web3.js";
import { lamportsToSolString } from "utils";
import { UnknownDetailsCard } from "components/instruction/UnknownDetailsCard";
import { SystemDetailsCard } from "components/instruction/system/SystemDetailsCard";
import { StakeDetailsCard } from "components/instruction/stake/StakeDetailsCard";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { TableCardBody } from "components/common/TableCardBody";
import { displayTimestamp } from "utils/date";
import { InfoTooltip } from "components/common/InfoTooltip";
import { Address } from "components/common/Address";
import { Signature } from "components/common/Signature";
import { intoTransactionInstruction } from "utils/tx";
import { TokenDetailsCard } from "components/instruction/token/TokenDetailsCard";
import { FetchStatus } from "providers/cache";

const AUTO_REFRESH_INTERVAL = 2000;
const ZERO_CONFIRMATION_BAILOUT = 5;

type SignatureProps = {
  signature: TransactionSignature;
};

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
  const [zeroConfirmationRetries, setZeroConfirmationRetries] = React.useState(
    0
  );

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
        <>
          <StatusCard signature={signature} autoRefresh={autoRefresh} />
          <AccountsCard signature={signature} autoRefresh={autoRefresh} />
          <InstructionsSection signature={signature} />
        </>
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
  const { firstAvailableBlock, status: clusterStatus } = useCluster();

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
    if (firstAvailableBlock !== undefined && firstAvailableBlock > 1) {
      return (
        <ErrorCard
          retry={() => fetchStatus(signature)}
          text="Not Found"
          subtext={`Note: Transactions processed before block ${firstAvailableBlock} are not available at this time`}
        />
      );
    }
    return <ErrorCard retry={() => fetchStatus(signature)} text="Not Found" />;
  }

  const { info } = status.data;

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

  const fee = details?.data?.transaction?.meta?.fee;
  const transaction = details?.data?.transaction?.transaction;
  const blockhash = transaction?.message.recentBlockhash;
  const isNonce = (() => {
    if (!transaction) return false;
    const ix = intoTransactionInstruction(transaction, 0);
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
        {autoRefresh === AutoRefresh.Active ? (
          <span className="spinner-grow spinner-grow-sm"></span>
        ) : (
          <button
            className="btn btn-white btn-sm"
            onClick={() => fetchStatus(signature)}
          >
            <span className="fe fe-refresh-cw mr-2"></span>
            Refresh
          </button>
        )}
      </div>

      <TableCardBody>
        <tr>
          <td>Signature</td>
          <td className="text-lg-right">
            <Signature signature={signature} alignRight />
          </td>
        </tr>

        <tr>
          <td>Result</td>
          <td className="text-lg-right">{renderResult()}</td>
        </tr>

        <tr>
          <td>Timestamp</td>
          <td className="text-lg-right">
            {info.timestamp !== "unavailable" ? (
              displayTimestamp(info.timestamp * 1000)
            ) : (
              <InfoTooltip
                bottom
                right
                text="Timestamps are available for confirmed blocks within the past 5 epochs"
              >
                Unavailable
              </InfoTooltip>
            )}
          </td>
        </tr>

        <tr>
          <td>Confirmations</td>
          <td className="text-lg-right text-uppercase">{info.confirmations}</td>
        </tr>

        <tr>
          <td>Block</td>
          <td className="text-lg-right">{info.slot}</td>
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
            <td className="text-lg-right">
              <code>{blockhash}</code>
            </td>
          </tr>
        )}

        {fee && (
          <tr>
            <td>Fee (SOL)</td>
            <td className="text-lg-right">{lamportsToSolString(fee)}</td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}

function AccountsCard({
  signature,
  autoRefresh,
}: SignatureProps & AutoRefreshProps) {
  const details = useTransactionDetails(signature);
  const fetchDetails = useFetchTransactionDetails();
  const fetchStatus = useFetchTransactionStatus();
  const refreshDetails = () => fetchDetails(signature);
  const refreshStatus = () => fetchStatus(signature);
  const transaction = details?.data?.transaction?.transaction;
  const message = transaction?.message;
  const status = useTransactionStatus(signature);

  // Fetch details on load
  React.useEffect(() => {
    if (status?.data?.info?.confirmations === "max" && !details) {
      fetchDetails(signature);
    }
  }, [signature, details, status, fetchDetails]);

  if (!status?.data?.info) {
    return null;
  } else if (autoRefresh === AutoRefresh.BailedOut) {
    return (
      <ErrorCard
        text="Details are not available until the transaction reaches MAX confirmations"
        retry={refreshStatus}
      />
    );
  } else if (autoRefresh === AutoRefresh.Active) {
    return (
      <ErrorCard text="Details are not available until the transaction reaches MAX confirmations" />
    );
  } else if (!details || details.status === FetchStatus.Fetching) {
    return <LoadingCard />;
  } else if (details.status === FetchStatus.FetchFailed) {
    return <ErrorCard retry={refreshDetails} text="Failed to fetch details" />;
  } else if (!details.data?.transaction || !message) {
    return <ErrorCard text="Details are not available" />;
  }

  const { meta } = details.data.transaction;
  if (!meta) {
    return <ErrorCard text="Transaction metadata is missing" />;
  }

  const accountRows = message.accountKeys.map((account, index) => {
    const pre = meta.preBalances[index];
    const post = meta.postBalances[index];
    const pubkey = account.pubkey;
    const key = account.pubkey.toBase58();
    const renderChange = () => {
      const change = post - pre;
      if (change === 0) return "";
      const sols = lamportsToSolString(change);
      if (change > 0) {
        return <span className="badge badge-soft-success">+{sols}</span>;
      } else {
        return <span className="badge badge-soft-warning">-{sols}</span>;
      }
    };

    return (
      <tr key={key}>
        <td>
          <Address pubkey={pubkey} link />
        </td>
        <td>{renderChange()}</td>
        <td>{lamportsToSolString(post)}</td>
        <td>
          {index === 0 && (
            <span className="badge badge-soft-info mr-1">Fee Payer</span>
          )}
          {!account.writable && (
            <span className="badge badge-soft-info mr-1">Readonly</span>
          )}
          {account.signer && (
            <span className="badge badge-soft-info mr-1">Signer</span>
          )}
          {message.instructions.find((ix) => ix.programId.equals(pubkey)) && (
            <span className="badge badge-soft-info mr-1">Program</span>
          )}
        </td>
      </tr>
    );
  });

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title">Account Inputs</h3>
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

function InstructionsSection({ signature }: SignatureProps) {
  const status = useTransactionStatus(signature);
  const details = useTransactionDetails(signature);
  const fetchDetails = useFetchTransactionDetails();
  const refreshDetails = () => fetchDetails(signature);

  if (!status?.data?.info || !details?.data?.transaction) return null;

  const { transaction } = details.data.transaction;
  if (transaction.message.instructions.length === 0) {
    return <ErrorCard retry={refreshDetails} text="No instructions found" />;
  }

  const result = status.data.info.result;
  const instructionDetails = transaction.message.instructions.map(
    (next, index) => {
      if ("parsed" in next) {
        if (next.program === "spl-token") {
          return (
            <TokenDetailsCard
              key={index}
              tx={transaction}
              ix={next}
              result={result}
              index={index}
            />
          );
        }

        const props = { ix: next, result, index };
        return <UnknownDetailsCard key={index} {...props} />;
      }

      const ix = intoTransactionInstruction(transaction, index);
      if (!ix) {
        return (
          <ErrorCard
            key={index}
            text="Could not display this instruction, please report"
          />
        );
      }

      const props = { ix, result, index };
      if (SystemProgram.programId.equals(ix.programId)) {
        return <SystemDetailsCard key={index} {...props} />;
      } else if (StakeProgram.programId.equals(ix.programId)) {
        return <StakeDetailsCard key={index} {...props} />;
      } else {
        return <UnknownDetailsCard key={index} {...props} />;
      }
    }
  );

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
