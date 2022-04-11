import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import { Idl, Program, BorshInstructionCoder } from "@project-serum/anchor";
import {
  getAnchorNameForInstruction,
  getProgramName,
  capitalizeFirstLetter,
  getAnchorAccountsFromInstruction,
} from "utils/anchor";
import { HexData } from "components/common/HexData";
import { Address } from "components/common/Address";
import ReactJson from "react-json-view";

export default function AnchorDetailsCard(props: {
  key: string;
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  signature: string;
  innerCards?: JSX.Element[];
  childIndex?: number;
  anchorProgram: Program<Idl>;
}) {
  const { ix, anchorProgram } = props;
  const programName = getProgramName(anchorProgram) ?? "Unknown Program";

  const ixName =
    getAnchorNameForInstruction(ix, anchorProgram) ?? "Unknown Instruction";
  const cardTitle = `${programName}: ${ixName}`;

  return (
    <InstructionCard title={cardTitle} {...props}>
      <RawAnchorDetails ix={ix} anchorProgram={anchorProgram} />
    </InstructionCard>
  );
}

function RawAnchorDetails({
  ix,
  anchorProgram,
}: {
  ix: TransactionInstruction;
  anchorProgram: Program;
}) {
  let ixAccounts:
    | {
        name: string;
        isMut: boolean;
        isSigner: boolean;
        pda?: Object;
      }[]
    | null = null;
  var decodedIxData = null;
  if (anchorProgram) {
    const decoder = new BorshInstructionCoder(anchorProgram.idl);
    decodedIxData = decoder.decode(ix.data);
    ixAccounts = getAnchorAccountsFromInstruction(decodedIxData, anchorProgram);
  }

  return (
    <>
      {ix.keys.map(({ pubkey, isSigner, isWritable }, keyIndex) => {
        return (
          <tr key={keyIndex}>
            <td>
              <div className="me-2 d-md-inline">
                {ixAccounts && keyIndex < ixAccounts.length
                  ? `${capitalizeFirstLetter(ixAccounts[keyIndex].name)}`
                  : `Account #${keyIndex + 1}`}
              </div>
              {isWritable && (
                <span className="badge bg-info-soft me-1">Writable</span>
              )}
              {isSigner && (
                <span className="badge bg-info-soft me-1">Signer</span>
              )}
            </td>
            <td className="text-lg-end">
              <Address pubkey={pubkey} alignRight link />
            </td>
          </tr>
        );
      })}

      <tr>
        <td>
          Instruction Data <span className="text-muted">(Hex)</span>
        </td>
        {decodedIxData ? (
          <td className="metadata-json-viewer m-4">
            <ReactJson src={decodedIxData} theme="solarized" />
          </td>
        ) : (
          <td className="text-lg-end">
            <HexData raw={ix.data} />
          </td>
        )}
      </tr>
    </>
  );
}
