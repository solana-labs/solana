import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { InstructionCard } from "../InstructionCard";
import { CancelSpotOrder, getSpotMarketFromInstruction } from "./types";

export function CancelSpotOrderDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: CancelSpotOrder;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;
  const mangoAccount = ix.keys[2];
  const mangoSpotMarket = getSpotMarketFromInstruction(ix, 4);

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Mango: CancelSpotOrder"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Mango account</td>
        <td>
          <Address pubkey={mangoAccount.pubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Spot market</td>
        <td className="text-lg-right">{mangoSpotMarket.name}</td>
      </tr>

      <tr>
        <td>Spot market address</td>
        <td>
          <Address pubkey={mangoSpotMarket.publicKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Order Id</td>
        <td className="text-lg-right">{info.orderId}</td>
      </tr>
    </InstructionCard>
  );
}
