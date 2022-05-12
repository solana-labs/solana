import React from "react";
import bs58 from "bs58";
import { CompiledInstruction, Message } from "@solana/web3.js";
import { TableCardBody } from "components/common/TableCardBody";
import { AddressWithContext, programValidator } from "./AddressWithContext";
import { useCluster } from "providers/cluster";
import { getProgramName } from "utils/tx";
import { HexData } from "components/common/HexData";
import getInstructionCardScrollAnchorId from "utils/get-instruction-card-scroll-anchor-id";
import { useScrollAnchor } from "providers/scroll-anchor";

export function InstructionsSection({ message }: { message: Message }) {
  return (
    <>
      {message.instructions.map((ix, index) => {
        return <InstructionCard key={index} {...{ message, ix, index }} />;
      })}
    </>
  );
}

function InstructionCard({
  message,
  ix,
  index,
}: {
  message: Message;
  ix: CompiledInstruction;
  index: number;
}) {
  const [expanded, setExpanded] = React.useState(false);
  const { cluster } = useCluster();
  const programId = message.accountKeys[ix.programIdIndex];
  const programName = getProgramName(programId.toBase58(), cluster);
  const scrollAnchorRef = useScrollAnchor(
    getInstructionCardScrollAnchorId([index + 1])
  );
  return (
    <div className="card" key={index} ref={scrollAnchorRef}>
      <div className={`card-header${!expanded ? " border-bottom-none" : ""}`}>
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          <span className={`badge bg-info-soft me-2`}>#{index + 1}</span>
          {programName} Instruction
        </h3>

        <button
          className={`btn btn-sm d-flex ${
            expanded ? "btn-black active" : "btn-white"
          }`}
          onClick={() => setExpanded((e) => !e)}
        >
          {expanded ? "Collapse" : "Expand"}
        </button>
      </div>
      {expanded && (
        <TableCardBody>
          <tr>
            <td>Program</td>
            <td className="text-lg-end">
              <AddressWithContext
                pubkey={message.accountKeys[ix.programIdIndex]}
                validator={programValidator}
              />
            </td>
          </tr>
          {ix.accounts.map((accountIndex, index) => {
            return (
              <tr key={index}>
                <td>
                  <div className="d-flex align-items-start flex-column">
                    Account #{index + 1}
                    <span className="mt-1">
                      {accountIndex < message.header.numRequiredSignatures && (
                        <span className="badge bg-info-soft me-2">Signer</span>
                      )}
                      {message.isAccountWritable(accountIndex) && (
                        <span className="badge bg-danger-soft me-2">
                          Writable
                        </span>
                      )}
                    </span>
                  </div>
                </td>
                <td className="text-lg-end">
                  <AddressWithContext
                    pubkey={message.accountKeys[accountIndex]}
                  />
                </td>
              </tr>
            );
          })}
          <tr>
            <td>
              Instruction Data <span className="text-muted">(Hex)</span>
            </td>
            <td className="text-lg-end">
              <HexData raw={bs58.decode(ix.data)} />
            </td>
          </tr>
        </TableCardBody>
      )}
    </div>
  );
}
