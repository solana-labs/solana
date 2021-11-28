import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import moment from "moment";
import { useCluster } from "providers/cluster";
import { useEffect, useState } from "react";
import { InstructionCard } from "../InstructionCard";
import {
  ChangePerpMarketParams,
  getPerpMarketFromInstruction,
  getPerpMarketFromPerpMarketConfig,
} from "./types";

export function ChangePerpMarketParamsDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: ChangePerpMarketParams;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  const perpMarketAccountMeta = ix.keys[1];
  const mangoPerpMarketConfig = getPerpMarketFromInstruction(
    ix,
    perpMarketAccountMeta
  );

  const cluster = useCluster();
  const [targetPeriodLength, setTargetPeriodLength] = useState<number | null>(
    null
  );
  useEffect(() => {
    async function getTargetPeriodLength() {
      if (mangoPerpMarketConfig === undefined) {
        return;
      }
      const mangoPerpMarket = await getPerpMarketFromPerpMarketConfig(
        cluster.url,
        mangoPerpMarketConfig
      );

      setTargetPeriodLength(
        mangoPerpMarket.liquidityMiningInfo.targetPeriodLength.toNumber()
      );
    }

    getTargetPeriodLength();
  }, [cluster.url, mangoPerpMarketConfig]);

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Mango Program: ChangePerpMarketParams"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      {info.initLeverageOption && (
        <tr>
          <td>Init leverage</td>
          <td className="text-lg-end">{info.initLeverage}</td>
        </tr>
      )}
      {info.liquidationFeeOption && (
        <tr>
          <td>Liquidation fee</td>
          <td className="text-lg-end">{info.liquidationFee}</td>
        </tr>
      )}
      {info.maintLeverageOption && (
        <tr>
          <td>Maint leverage</td>
          <td className="text-lg-end">{info.maintLeverage}</td>
        </tr>
      )}
      {info.makerFeeOption && (
        <tr>
          <td>Maker fee</td>
          <td className="text-lg-end">{info.makerFee}</td>
        </tr>
      )}
      {info.mngoPerPeriodOption && (
        <tr>
          <td>
            MNGO per{" "}
            {targetPeriodLength !== null &&
              moment.duration(targetPeriodLength, "seconds").humanize()}
          </td>
          <td className="text-lg-end">
            {info.mngoPerPeriod} {}
          </td>
        </tr>
      )}

      {info.maxDepthBpsOption && (
        <tr>
          <td>Max depth bps</td>
          <td className="text-lg-end">{info.maxDepthBps}</td>
        </tr>
      )}
      {info.rateOption && (
        <tr>
          <td>Rate</td>
          <td className="text-lg-end">{info.rate}</td>
        </tr>
      )}
      {info.takerFeeOption && (
        <tr>
          <td>Taker fee</td>
          <td className="text-lg-end">{info.takerFee}</td>
        </tr>
      )}
      {info.targetPeriodLengthOption && (
        <tr>
          <td>Target period length</td>
          <td className="text-lg-end">{info.targetPeriodLength}</td>
        </tr>
      )}
    </InstructionCard>
  );
}
