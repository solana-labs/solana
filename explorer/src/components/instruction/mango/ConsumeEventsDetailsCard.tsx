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

  const mangoPerpMarket = getPerpMarketFromInstruction(ix, 2);

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={"Mango: ConsumeEvents"}
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Perp market</td>
        <td className="text-lg-right">{mangoPerpMarket.name}</td>
      </tr>

      <tr>
        <td>Perp market address</td>
        <td>
          <Address pubkey={mangoPerpMarket.publicKey} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
