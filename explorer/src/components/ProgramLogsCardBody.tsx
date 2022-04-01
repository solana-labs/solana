import { Message, ParsedMessage, PublicKey } from "@solana/web3.js";
import { Cluster } from "providers/cluster";
import { TableCardBody } from "components/common/TableCardBody";
import { InstructionLogs } from "utils/program-logs";
import { ProgramName } from "utils/anchor";

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

        return (<ProgramInstructionLog key={index} index={index} programId={programId} programLogs={programLogs} cluster={cluster} />);
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
  let badgeColor = "white";
  if (programLogs) {
    badgeColor = programLogs.failed ? "warning" : "success";
  }

  return (
    <tr key={index}>
      <td>
        <div className="d-flex align-items-center">
          <span className={`badge bg-${badgeColor}-soft me-2`}>
            #{index + 1}
          </span>
          <ProgramName programId={programId} cluster={cluster} /> Instruction
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