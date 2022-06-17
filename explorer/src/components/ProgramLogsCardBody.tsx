import { Message, ParsedMessage } from "@solana/web3.js";
import { Cluster } from "src/providers/cluster";
import { TableCardBody } from "src/components/common/TableCardBody";
import { InstructionLogs } from "src/utils/program-logs";
import { ProgramName } from "src/utils/anchor";
import React from "react";
import Link from "next/link";
import getInstructionCardScrollAnchorId from "src/utils/get-instruction-card-scroll-anchor-id";

const NATIVE_PROGRAMS_MISSING_INVOKE_LOG: string[] = [
  "AddressLookupTab1e1111111111111111111111111",
  "ZkTokenProof1111111111111111111111111111111",
  "BPFLoader1111111111111111111111111111111111",
  "BPFLoader2111111111111111111111111111111111",
  "BPFLoaderUpgradeab1e11111111111111111111111",
];

export function ProgramLogsCardBody({
  message,
  logs,
  cluster,
  url,
}: {
  message: Message | ParsedMessage;
  logs: InstructionLogs[];
  cluster: Cluster;
  url: string;
}) {
  let logIndex = 0;

  const jumpToLocation = (index: number) =>
    `#${getInstructionCardScrollAnchorId([index + 1])}`

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

        const programAddress = programId.toBase58();
        let programLogs: InstructionLogs | undefined = logs[logIndex];
        if (programLogs?.invokedProgram === programAddress) {
          logIndex++;
        } else if (
          programLogs?.invokedProgram === null &&
          programLogs.logs.length > 0 &&
          NATIVE_PROGRAMS_MISSING_INVOKE_LOG.includes(programAddress)
        ) {
          logIndex++;
        } else {
          programLogs = undefined;
        }

        let badgeColor = "white";
        if (programLogs) {
          badgeColor = programLogs.failed ? "warning" : "success";
        }

        return (
          <tr key={index}>
            <td>
              <Link href={jumpToLocation(index)} passHref>
                <a>
                  <div className="d-flex align-items-center">
                    <span className={`badge bg-${badgeColor}-soft me-2`}>
                      #{index + 1}
                    </span>
                    <span className="program-log-instruction-name">
                      <ProgramName
                        programId={programId}
                        cluster={cluster}
                        url={url}
                      />{" "}
                      Instruction
                    </span>
                    <span className="fe fe-chevrons-up c-pointer px-2" />
                  </div>
                </a>
              </Link>
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
      })}
    </TableCardBody>
  );
}
