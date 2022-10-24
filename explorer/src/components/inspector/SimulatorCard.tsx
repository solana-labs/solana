import React from "react";
import {
  Connection,
  VersionedMessage,
  VersionedTransaction,
} from "@solana/web3.js";
import { useCluster } from "providers/cluster";
import { InstructionLogs, parseProgramLogs } from "utils/program-logs";
import { ProgramLogsCardBody } from "components/ProgramLogsCardBody";

export function SimulatorCard({ message }: { message: VersionedMessage }) {
  const { cluster, url } = useCluster();
  const {
    simulate,
    simulating,
    simulationLogs: logs,
    simulationError,
  } = useSimulator(message);
  if (simulating) {
    return (
      <div className="card">
        <div className="card-header">
          <h3 className="card-header-title">Transaction Simulation</h3>
        </div>
        <div className="card-body text-center">
          <span className="spinner-grow spinner-grow-sm me-2"></span>
          Simulating
        </div>
      </div>
    );
  } else if (!logs) {
    return (
      <div className="card">
        <div className="card-header">
          <h3 className="card-header-title">Transaction Simulation</h3>
          <button className="btn btn-sm d-flex btn-white" onClick={simulate}>
            {simulationError ? "Retry" : "Simulate"}
          </button>
        </div>
        <div className="card-body">
          {simulationError ? (
            <>
              Simulation Failure:
              <span className="text-warning ms-2">{simulationError}</span>
            </>
          ) : (
            <ul className="text-muted">
              <li>
                Simulation is free and will run this transaction against the
                latest confirmed ledger state.
              </li>
              <li>
                No state changes will be persisted and all signature checks will
                be disabled.
              </li>
            </ul>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title">Transaction Simulation</h3>
        <button className="btn btn-sm d-flex btn-white" onClick={simulate}>
          Retry
        </button>
      </div>
      <ProgramLogsCardBody
        message={message}
        logs={logs}
        cluster={cluster}
        url={url}
      />
    </div>
  );
}

function useSimulator(message: VersionedMessage) {
  const { cluster, url } = useCluster();
  const [simulating, setSimulating] = React.useState(false);
  const [logs, setLogs] = React.useState<Array<InstructionLogs> | null>(null);
  const [error, setError] = React.useState<string>();

  React.useEffect(() => {
    setLogs(null);
    setSimulating(false);
    setError(undefined);
  }, [url]);

  const onClick = React.useCallback(() => {
    if (simulating) return;
    setError(undefined);
    setSimulating(true);

    const connection = new Connection(url, "confirmed");
    (async () => {
      try {
        // Simulate without signers to skip signer verification
        const resp = await connection.simulateTransaction(
          new VersionedTransaction(message),
          { replaceRecentBlockhash: true }
        );

        if (resp.value.logs === null) {
          throw new Error("Expected to receive logs from simulation");
        }

        if (
          resp.value.logs.length === 0 &&
          typeof resp.value.err === "string"
        ) {
          setLogs(null);
          setError(resp.value.err);
        } else {
          // Prettify logs
          setLogs(parseProgramLogs(resp.value.logs, resp.value.err, cluster));
        }
      } catch (err) {
        console.error(err);
        setLogs(null);
        if (err instanceof Error) {
          setError(err.message);
        }
      } finally {
        setSimulating(false);
      }
    })();
  }, [cluster, url, message, simulating]);
  return {
    simulate: onClick,
    simulating,
    simulationLogs: logs,
    simulationError: error,
  };
}
