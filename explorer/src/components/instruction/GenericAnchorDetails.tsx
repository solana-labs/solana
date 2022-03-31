import {
  SignatureResult,
  TransactionInstruction,
} from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import {
  BorshInstructionCoder,
  Idl,
  Program,
} from "@project-serum/anchor";
import { useMemo } from "react";
import { Address } from "../common/Address";
import { snakeCase } from "snake-case";

export function GenericAnchorDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  signature: string;
  innerCards?: JSX.Element[];
  childIndex?: number;
  program: Program<Idl>
}) {
  const { ix, index, result, innerCards, childIndex, program } = props;

  const idl = program.idl;
  const renderProps = useMemo(() => {
    // e.g. voter stake registry -> Voter Stake Registry
    var _programName = program.idl.name.replaceAll("_", " ").trim();
    _programName = _programName
      .toLowerCase()
      .split(" ")
      .map((word) => word.charAt(0).toUpperCase() + word.substring(1))
      .join(" ");
    const programName = _programName.charAt(0).toUpperCase() + _programName.slice(1);

    const coder = new BorshInstructionCoder(idl);
    const decodedIx = coder.decode(ix.data);

    if (!decodedIx) {
      return null;
    }

    // get ix title, pascal case it
    var _ixTitle = decodedIx.name;
    const ixTitle = _ixTitle.charAt(0).toUpperCase() + _ixTitle.slice(1);

    // get ix accounts
    const idlInstructions = idl.instructions.filter(
      (ix) => ix.name === decodedIx.name
    );
    if (idlInstructions.length === 0) {
      return null;
    }
    const ixAccounts = idlInstructions[0].accounts as {
        // type coercing since anchor doesn't export the underlying type
        name: string;
        isMut: boolean;
        isSigner: boolean;
        pda?: Object;
    }[];

    return { ixTitle, ixAccounts, programName }
  }, [ix.data, program, idl]);

  if (!renderProps) {
    throw new Error("Failed to deserialize instruction data");
  }

  const { ixTitle, ixAccounts, programName } = renderProps;

  return (
    <div>
      {idl && (
        <InstructionCard
          ix={ix}
          index={index}
          result={result}
          title={`${programName || "Unknown"}: ${ixTitle || "Unknown"}`}
          innerCards={innerCards}
          childIndex={childIndex}
          defaultRaw
        >
          <tr key={ix.programId.toBase58()}>
            <td>Program</td>
            <td className="text-lg-end">
              <Address pubkey={ix.programId} alignRight link />
            </td>
          </tr>

          {ixAccounts != null &&
            ix.keys.map((am, keyIndex) => (
              <tr key={keyIndex}>
                <td>
                  <div className="me-2 d-md-inline">
                    {/* remaining accounts would not have a name */}
                    {ixAccounts[keyIndex] &&
                      snakeCase(ixAccounts[keyIndex].name)}
                    {!ixAccounts[keyIndex] &&
                      "remaining account #" +
                        (keyIndex - ixAccounts.length + 1)}
                  </div>
                  {am.isWritable && (
                    <span className="badge bg-info-soft me-1">Writable</span>
                  )}
                  {am.isSigner && (
                    <span className="badge bg-info-soft me-1">Signer</span>
                  )}
                </td>
                <td>
                  <Address pubkey={am.pubkey} alignRight link />
                </td>
              </tr>
            ))}
        </InstructionCard>
      )}
      {!idl && (
        <InstructionCard
          ix={ix}
          index={index}
          result={result}
          title={`Unknown Program: Unknown Instruction`}
          innerCards={innerCards}
          childIndex={childIndex}
          defaultRaw
        />
      )}
    </div>
  );
}
