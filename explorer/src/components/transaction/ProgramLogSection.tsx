import React from "react";
import { SignatureProps } from "pages/TransactionDetailsPage";
import { useTransactionDetails } from "providers/transactions";
import { ProgramLogsCardBody } from "components/ProgramLogsCardBody";
import { parseProgramLogs } from "utils/program-logs";
import { useCluster } from "providers/cluster";
import { TableCardBody } from "components/common/TableCardBody";

export function ProgramLogSection({ signature }: SignatureProps) {
  const [showRaw, setShowRaw] = React.useState(false);
  const { cluster, url } = useCluster();
  const details = useTransactionDetails(signature);

  const transactionWithMeta = details?.data?.transactionWithMeta;
  if (!transactionWithMeta) return null;
  const message = transactionWithMeta.transaction.message;

  const logMessages = transactionWithMeta.meta?.logMessages || null;
  const err = transactionWithMeta.meta?.err || null;

  let prettyLogs = null;
  if (logMessages !== null) {
    prettyLogs = parseProgramLogs(logMessages, err, cluster);
  }

  return (
    <>
      <div className="card">
        <div className="card-header">
          <h3 className="card-header-title">Program Instruction Logs</h3>
          <button
            className={`btn btn-sm d-flex ${
              showRaw ? "btn-black active" : "btn-white"
            }`}
            onClick={() => setShowRaw((r) => !r)}
          >
            <span className="fe fe-code me-1"></span>
            Raw
          </button>
        </div>
        {prettyLogs !== null ? (
          showRaw ? (
            <RawProgramLogs raw={logMessages!} />
          ) : (
            <ProgramLogsCardBody
              message={message}
              logs={prettyLogs}
              cluster={cluster}
              url={url}
            />
          )
        ) : (
          <div className="card-body">
            Logs not supported for this transaction
          </div>
        )}
      </div>
    </>
  );
}

const RawProgramLogs = ({ raw }: { raw: string[] }) => {
  return (
    <TableCardBody>
      <tr>
        <td>
          <pre className="text-start">{JSON.stringify(raw, null, 2)}</pre>
        </td>
      </tr>
    </TableCardBody>
  );
};
