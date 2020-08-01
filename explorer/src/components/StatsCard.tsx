import React from "react";
import CountUp from "react-countup";

import TableCardBody from "./common/TableCardBody";
import {
  useDashboardInfo,
  usePerformanceInfo,
  useRootSlot,
  PERF_UPDATE_SEC,
  useSetActive,
} from "providers/stats/solanaBeach";
import { slotsToHumanString } from "utils";
import { useCluster, Cluster } from "providers/cluster";

export default function StatsCard() {
  return (
    <div className="card">
      <div className="card-header">
        <div className="row align-items-center">
          <div className="col">
            <h4 className="card-header-title">Live Cluster Info</h4>
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
  const txTrackerRef = React.useRef({ old: 0, new: 0 });
  const txTracker = txTrackerRef.current;
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

  if (performanceInfo) {
    const { totalTransactionCount: txCount, avgTPS } = performanceInfo;

    // Track last tx count to initialize count up
    if (txCount !== txTracker.new) {
      // If this is the first tx count value, estimate the previous one
      // in order to have a starting point for our animation
      txTracker.old = txTracker.new || txCount - PERF_UPDATE_SEC * avgTPS;
      txTracker.new = txCount;
    }
  } else {
    txTrackerRef.current = { old: 0, new: 0 };
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
  const transactionCount = (
    <CountUp
      start={txTracker.old}
      end={txTracker.new}
      duration={PERF_UPDATE_SEC + 2}
      delay={0}
      useEasing={false}
      preserveValue={true}
      separator=","
    />
  );
  const averageTps = Math.round(performanceInfo.avgTPS);

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
