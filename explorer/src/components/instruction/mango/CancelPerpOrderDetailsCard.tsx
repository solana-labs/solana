import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { InstructionCard } from "../InstructionCard";
import { CancelPerpOrder, getPerpMarketFromInstruction } from "./types";

export function CancelPerpOrderDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: CancelPerpOrder;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;
  const mangoAccount = ix.keys[1];
  const mangoPerpMarket = getPerpMarketFromInstruction(ix, 3);

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Mango: CancelPerpOrder"
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
        <td>Perp market</td>
        <td className="text-lg-right">{mangoPerpMarket.name}</td>
      </tr>

      <tr>
        <td>Perp market address</td>
        <td>
          <Address pubkey={mangoPerpMarket.publicKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Order Id</td>
        <td className="text-lg-right">{info.orderId}</td>
      </tr>
    </InstructionCard>
  );
}
