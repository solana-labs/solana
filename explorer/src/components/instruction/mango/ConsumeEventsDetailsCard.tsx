import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { InstructionCard } from "../InstructionCard";
import { getPerpMarketFromInstruction } from "./types";

export function ConsumeEventsDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, innerCards, childIndex } = props;

  const perpMarketAccountMeta = ix.keys[2];
  const mangoPerpMarketConfig = getPerpMarketFromInstruction(
    ix,
    perpMarketAccountMeta
  );

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={"Mango Program: ConsumeEvents"}
      innerCards={innerCards}
      childIndex={childIndex}
    >
      {mangoPerpMarketConfig !== undefined && (
        <tr>
          <td>Perp market</td>
          <td className="text-lg-end">{mangoPerpMarketConfig.name}</td>
        </tr>
      )}

      <tr>
        <td>Perp market address</td>
        <td>
          <Address pubkey={perpMarketAccountMeta.pubkey} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
