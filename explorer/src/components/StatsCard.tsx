import React from "react";
import CountUp from "react-countup";

import TableCardBody from "./common/TableCardBody";
import {
  useDashboardInfo,
  usePerformanceInfo,
  useRootSlot,
  PERF_UPDATE_SEC,
  useSetActive,
  PerformanceInfo,
} from "providers/stats/solanaBeach";
import { slotsToHumanString } from "utils";
import { useCluster, Cluster } from "providers/cluster";

export default function StatsCard() {
  return (
    <div className="card">
      <div className="card-header">
        <div className="row align-items-center">
          <div className="col">
            <h4 className="card-header-title">Live Cluster Stats</h4>
          </div>
        </div>
      </div>
      <StatsCardBody />
    </div>
  );
}

function StatsCardBody() {
  const rootSlot = useRootSlot();
  const dashboardInfo = useDashboardInfo();
  const performanceInfo = usePerformanceInfo();
  const setSocketActive = useSetActive();
  const { cluster } = useCluster();

  React.useEffect(() => {
    setSocketActive(true);
    return () => setSocketActive(false);
  }, [setSocketActive, cluster]);

  const statsAvailable =
    cluster === Cluster.MainnetBeta || cluster === Cluster.Testnet;
  if (!statsAvailable) {
    return (
      <div className="card-body text-center">
        <div className="text-muted">
          Stats are not available for this cluster
        </div>
      </div>
    );
  }

  if (rootSlot === undefined || !dashboardInfo || !performanceInfo) {
    return (
      <div className="card-body text-center">
        <span className="spinner-grow spinner-grow-sm mr-2"></span>
        Loading
      </div>
    );
  }

  const currentBlock = rootSlot.toLocaleString("en-US");
  const { avgBlockTime_1min, epochInfo } = dashboardInfo;
  const averageBlockTime = Math.round(1000 * avgBlockTime_1min) + "ms";
  const { slotIndex, slotsInEpoch } = epochInfo;
  const currentEpoch = epochInfo.epoch.toString();
  const epochProgress = ((100 * slotIndex) / slotsInEpoch).toFixed(1) + "%";
  const epochTimeRemaining = slotsToHumanString(slotsInEpoch - slotIndex);
  const averageTps = Math.round(performanceInfo.avgTPS);
  const transactionCount = <AnimatedTransactionCount info={performanceInfo} />;

  return (
    <TableCardBody>
      <tr>
        <td className="w-100">Block</td>
        <td className="text-right text-monospace">{currentBlock}</td>
      </tr>
      <tr>
        <td className="w-100">Block time</td>
        <td className="text-right text-monospace">{averageBlockTime}</td>
      </tr>
      <tr>
        <td className="w-100">Epoch</td>
        <td className="text-right text-monospace">{currentEpoch} </td>
      </tr>
      <tr>
        <td className="w-100">Epoch progress</td>
        <td className="text-right text-monospace">{epochProgress} </td>
      </tr>
      <tr>
        <td className="w-100">Epoch time remaining</td>
        <td className="text-right text-monospace">{epochTimeRemaining} </td>
      </tr>
      <tr>
        <td className="w-100">Transaction count</td>
        <td className="text-right text-monospace">{transactionCount} </td>
      </tr>
      <tr>
        <td className="w-100">Transactions per second</td>
        <td className="text-right text-monospace">{averageTps} </td>
      </tr>
    </TableCardBody>
  );
}

function AnimatedTransactionCount({ info }: { info: PerformanceInfo }) {
  const txCountRef = React.useRef(0);
  const countUpRef = React.useRef({ start: 0, period: 0, lastUpdate: 0 });
  const countUp = countUpRef.current;

  const { totalTransactionCount: txCount, avgTPS } = info;

  // Track last tx count to reset count up options
  if (txCount !== txCountRef.current) {
    if (countUp.lastUpdate > 0) {
      // Since we overshoot below, calculate the elapsed value
      // and start from there.
      const elapsed = Date.now() - countUp.lastUpdate;
      const elapsedPeriods = elapsed / (PERF_UPDATE_SEC * 1000);
      countUp.start = countUp.start + elapsedPeriods * countUp.period;
      countUp.period = txCount - countUp.start;
    } else {
      // Since this is the first tx count value, estimate the previous
      // tx count in order to have a starting point for our animation
      countUp.period = PERF_UPDATE_SEC * avgTPS;
      countUp.start = txCount - countUp.period;
    }
    countUp.lastUpdate = Date.now();
    txCountRef.current = txCount;
  }

  // Overshoot the target tx count in case the next update is delayed
  const COUNT_PERIODS = 3;
  const countUpEnd = countUp.start + COUNT_PERIODS * countUp.period;
  return (
    <CountUp
      start={countUp.start}
      end={countUpEnd}
      duration={PERF_UPDATE_SEC * COUNT_PERIODS}
      delay={0}
      useEasing={false}
      preserveValue={true}
      separator=","
    />
  );
}
