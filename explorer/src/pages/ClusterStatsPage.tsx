import React from "react";

import { TableCardBody } from "components/common/TableCardBody";
import {
  useDashboardInfo,
  usePerformanceInfo,
  useSetActive,
} from "providers/stats/solanaBeach";
import { slotsToHumanString } from "utils";
import { useCluster, Cluster } from "providers/cluster";
import { TpsCard } from "components/TpsCard";

export function ClusterStatsPage() {
  return (
    <div className="container mt-4">
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
      <TpsCard />
    </div>
  );
}

function StatsCardBody() {
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

  if (!dashboardInfo || !performanceInfo) {
    return (
      <div className="card-body text-center">
        <span className="spinner-grow spinner-grow-sm mr-2"></span>
        Loading
      </div>
    );
  }

  const { avgBlockTime_1h, avgBlockTime_1min, epochInfo } = dashboardInfo;
  const hourlyBlockTime = Math.round(1000 * avgBlockTime_1h);
  const averageBlockTime = Math.round(1000 * avgBlockTime_1min) + "ms";
  const { slotIndex, slotsInEpoch } = epochInfo;
  const currentEpoch = epochInfo.epoch.toString();
  const epochProgress = ((100 * slotIndex) / slotsInEpoch).toFixed(1) + "%";
  const epochTimeRemaining = slotsToHumanString(
    slotsInEpoch - slotIndex,
    hourlyBlockTime
  );
  const blockHeight = epochInfo.blockHeight.toLocaleString("en-US");
  const currentSlot = epochInfo.absoluteSlot.toLocaleString("en-US");

  return (
    <TableCardBody>
      <tr>
        <td className="w-100">Slot</td>
        <td className="text-lg-right text-monospace">{currentSlot}</td>
      </tr>
      <tr>
        <td className="w-100">Block height</td>
        <td className="text-lg-right text-monospace">{blockHeight}</td>
      </tr>
      <tr>
        <td className="w-100">Block time</td>
        <td className="text-lg-right text-monospace">{averageBlockTime}</td>
      </tr>
      <tr>
        <td className="w-100">Epoch</td>
        <td className="text-lg-right text-monospace">{currentEpoch} </td>
      </tr>
      <tr>
        <td className="w-100">Epoch progress</td>
        <td className="text-lg-right text-monospace">{epochProgress} </td>
      </tr>
      <tr>
        <td className="w-100">Epoch time remaining</td>
        <td className="text-lg-right text-monospace">{epochTimeRemaining} </td>
      </tr>
    </TableCardBody>
  );
}
