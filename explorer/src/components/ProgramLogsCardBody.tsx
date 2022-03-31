import { Message, ParsedMessage, PublicKey } from "@solana/web3.js";
import { Cluster, clusterUrl, DEFAULT_CUSTOM_URL } from "providers/cluster";
import { TableCardBody } from "components/common/TableCardBody";
import { programLabel } from "utils/tx";
import { InstructionLogs } from "utils/program-logs";
import { useAnchorProgram } from "providers/anchor";
import { Program } from "@project-serum/anchor";
import { getProgramName } from "utils/anchor";

export function ProgramLogsCardBody({
  message,
  logs,
  cluster,
}: {
  message: Message | ParsedMessage;
  logs: InstructionLogs[];
  cluster: Cluster;
}) {
  return (
    <TableCardBody>
      {message.instructions.map((ix, index) => {
        let programId;
        if ("programIdIndex" in ix) {
          const programAccount = message.accountKeys[ix.programIdIndex];
          if ("pubkey" in programAccount) {
            programId = programAccount.pubkey;
          } else {
            programId = programAccount;
          }
        } else {
          programId = ix.programId;
        }
        const programLogs: InstructionLogs | undefined = logs[index];

        return (<ProgramInstructionLog index={index} programId={programId} programLogs={programLogs} cluster={cluster} />);
      })}
    </TableCardBody>
  );
}

function ProgramInstructionLog({ index, programId, programLogs, cluster }: {
  index: number,
  programId: PublicKey,
  programLogs: InstructionLogs | undefined,
  cluster: Cluster
}) {
  let url = clusterUrl(cluster, DEFAULT_CUSTOM_URL);
  const program = useAnchorProgram(programId.toString(), url);

  let badgeColor = "white";
  if (programLogs) {
    badgeColor = programLogs.failed ? "warning" : "success";
  }

  const programName =
    getProgramName(program) ?? (programLabel(programId.toBase58(), cluster) || "Unknown Program");

  return (
    <tr key={index}>
      <td>
        <div className="d-flex align-items-center">
          <span className={`badge bg-${badgeColor}-soft me-2`}>
            #{index + 1}
          </span>
          {programName} Instruction
        </div>
        {programLogs && (
          <div className="d-flex align-items-start flex-column font-monospace p-2 font-size-sm">
            {programLogs.logs.map((log, key) => {
              return (
                <span key={key}>
                  <span className="text-muted">{log.prefix}</span>
                  <span className={`text-${log.style}`}>{log.text}</span>
                </span>
              );
            })}
          </div>
        )}
      </td>
    </tr>
  );
}