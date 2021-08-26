import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { InstructionCard } from "../InstructionCard";
import { getSpotMarketFromInstruction } from "./types";

export function GenericSpotMngoDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  accountKeyLocation: number;
  spotMarketkeyLocation: number;
  title: String;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const {
    ix,
    index,
    result,
    accountKeyLocation,
    spotMarketkeyLocation,
    title,
    innerCards,
    childIndex,
  } = props;
  const mangoAccount = ix.keys[accountKeyLocation];
  const mangoSpotMarket = getSpotMarketFromInstruction(
    ix,
    spotMarketkeyLocation
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
        <td>Spot market</td>
        <td className="text-lg-right">{mangoSpotMarket.name}</td>
      </tr>

      <tr>
        <td>Spot market address</td>
        <td>
          <Address pubkey={mangoSpotMarket.publicKey} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
