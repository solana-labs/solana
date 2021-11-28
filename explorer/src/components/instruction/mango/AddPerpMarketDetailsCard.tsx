import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import moment from "moment";
import { InstructionCard } from "../InstructionCard";
import { AddPerpMarket } from "./types";

export function AddPerpMarketDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: AddPerpMarket;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Mango Program: AddPerpMarket"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Market index</td>
        <td className="text-lg-end">{info.marketIndex}</td>
      </tr>
      <tr>
        <td>Maintenance leverage</td>
        <td className="text-lg-end">{info.maintLeverage}</td>
      </tr>
      <tr>
        <td>Initial leverage</td>
        <td className="text-lg-end">{info.initLeverage}</td>
      </tr>
      <tr>
        <td>Liquidation fee</td>
        <td className="text-lg-end">{info.liquidationFee}</td>
      </tr>
      <tr>
        <td>Maker fee</td>
        <td className="text-lg-end">{info.makerFee}</td>
      </tr>
      <tr>
        <td>Taker fee</td>
        <td className="text-lg-end">{info.takerFee}</td>
      </tr>
      <tr>
        <td>Base lot size</td>
        <td className="text-lg-end">{info.baseLotSize}</td>
      </tr>
      <tr>
        <td>Quote lot size</td>
        <td className="text-lg-end">{info.quoteLotSize}</td>
      </tr>
      <tr>
        <td>Rate</td>
        <td className="text-lg-end">{info.rate}</td>
      </tr>
      <tr>
        <td>Max depth bps</td>
        <td className="text-lg-end">{info.maxDepthBps}</td>
      </tr>
      <tr>
        <td>
          MNGO per{" "}
          {moment.duration(info.targetPeriodLength, "seconds").humanize()}
        </td>
        <td className="text-lg-end">
          {info.mngoPerPeriod} {}
        </td>
      </tr>
    </InstructionCard>
  );
}
