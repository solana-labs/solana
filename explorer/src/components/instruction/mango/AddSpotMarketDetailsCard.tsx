import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { AddSpotMarket, spotMarketFromIndex } from "./types";

export function AddSpotMarketDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: AddSpotMarket;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Mango Program: AddSpotMarket"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      {spotMarketFromIndex(ix, info.marketIndex) !== "UNKNOWN" && (
        <tr>
          <td>Market</td>
          <td className="text-lg-end">
            {spotMarketFromIndex(ix, info.marketIndex)}
          </td>
        </tr>
      )}
      <tr>
        <td>Market index</td>
        <td className="text-lg-end">{info.marketIndex}</td>
      </tr>
      <tr>
        <td>Maint leverage</td>
        <td className="text-lg-end">{info.maintLeverage}</td>
      </tr>
      <tr>
        <td>Init leverage</td>
        <td className="text-lg-end">{info.initLeverage}</td>
      </tr>
      <tr>
        <td>Liquidation fee</td>
        <td className="text-lg-end">{info.liquidationFee}</td>
      </tr>
      <tr>
        <td>Optimal util</td>
        <td className="text-lg-end">{info.optimalUtil}</td>
      </tr>
      <tr>
        <td>Optimal rate</td>
        <td className="text-lg-end">{info.optimalRate}</td>
      </tr>
      <tr>
        <td>Max rate</td>
        <td className="text-lg-end">{info.maxRate}</td>
      </tr>
    </InstructionCard>
  );
}
