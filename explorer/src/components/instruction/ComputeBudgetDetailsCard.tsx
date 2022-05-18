import React from "react";
import {
  ComputeBudgetInstruction,
  SignatureResult,
  TransactionInstruction,
} from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import { SolBalance } from "utils";
import { Address } from "components/common/Address";
import { reportError } from "utils/sentry";
import { useCluster } from "providers/cluster";

export function ComputeBudgetDetailsCard({
  ix,
  index,
  result,
  signature,
  innerCards,
  childIndex,
}: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  signature: string;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { url } = useCluster();
  try {
    const type = ComputeBudgetInstruction.decodeInstructionType(ix);
    switch (type) {
      case "RequestUnits": {
        const { units, additionalFee } =
          ComputeBudgetInstruction.decodeRequestUnits(ix);
        return (
          <InstructionCard
            ix={ix}
            index={index}
            result={result}
            title="Compute Budget Program: Request Units"
            innerCards={innerCards}
            childIndex={childIndex}
          >
            <tr>
              <td>Program</td>
              <td className="text-lg-end">
                <Address pubkey={ix.programId} alignRight link />
              </td>
            </tr>

            <tr>
              <td>Requested Compute Units</td>
              <td className="text-lg-end">{units}</td>
            </tr>

            <tr>
              <td>Additional Fee (SOL)</td>
              <td className="text-lg-end">
                <SolBalance lamports={additionalFee} />
              </td>
            </tr>
          </InstructionCard>
        );
      }
      case "RequestHeapFrame": {
        const { bytes } = ComputeBudgetInstruction.decodeRequestHeapFrame(ix);
        return (
          <InstructionCard
            ix={ix}
            index={index}
            result={result}
            title="Compute Budget Program: Request Heap Frame"
            innerCards={innerCards}
            childIndex={childIndex}
          >
            <tr>
              <td>Program</td>
              <td className="text-lg-end">
                <Address pubkey={ix.programId} alignRight link />
              </td>
            </tr>

            <tr>
              <td>Requested Heap Frame (Bytes)</td>
              <td className="text-lg-end">{bytes}</td>
            </tr>
          </InstructionCard>
        );
      }
    }
  } catch (error) {
    reportError(error, {
      url: url,
      signature: signature,
    });
  }

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Compute Budget Program: Unknown Instruction"
      innerCards={innerCards}
      childIndex={childIndex}
      defaultRaw
    />
  );
}
