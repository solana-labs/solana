import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { InstructionCard } from "../InstructionCard";
import { getPerpMarketFromInstruction } from "./types";

export function GenericPerpMngoDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  mangoAccountKeyLocation: number;
  perpMarketKeyLocation: number;
  title: String;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const {
    ix,
    index,
    result,
    mangoAccountKeyLocation,
    perpMarketKeyLocation,
    title,
    innerCards,
    childIndex,
  } = props;
  const mangoAccount = ix.keys[mangoAccountKeyLocation];
  const mangoPerpMarket = getPerpMarketFromInstruction(
    ix,
    perpMarketKeyLocation
  );

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={"Mango: " + title}
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
    </InstructionCard>
  );
}
