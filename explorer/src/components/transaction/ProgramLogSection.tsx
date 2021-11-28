import React from "react";
import { SignatureProps } from "pages/TransactionDetailsPage";
import { useTransactionDetails } from "providers/transactions";
import { ProgramLogsCardBody } from "components/ProgramLogsCardBody";
import { prettyProgramLogs } from "utils/program-logs";
import { useCluster } from "providers/cluster";

export function ProgramLogSection({ signature }: SignatureProps) {
  const { cluster } = useCluster();
  const details = useTransactionDetails(signature);

  const transaction = details?.data?.transaction;
  if (!transaction) return null;
  const message = transaction.transaction.message;

  const logMessages = transaction.meta?.logMessages || null;
  const err = transaction.meta?.err || null;
  const prettyLogs = prettyProgramLogs(logMessages, err, cluster);

  return (
    <>
      <div className="card">
        <div className="card-header">
          <h3 className="card-header-title">Program Logs</h3>
        </div>
        <ProgramLogsCardBody
          message={message}
          logs={prettyLogs}
          cluster={cluster}
        />
      </div>
    </>
  );
}
