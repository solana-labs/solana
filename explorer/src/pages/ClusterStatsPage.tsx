import React from "react";
import { TableCardBody } from "components/common/TableCardBody";
import { Slot } from "components/common/Slot";
import {
  ClusterStatsStatus,
  useDashboardInfo,
  usePerformanceInfo,
  useStatsProvider,
} from "providers/stats/solanaClusterStats";
import { slotsToHumanString } from "utils";
import { useCluster } from "providers/cluster";
import { TpsCard } from "components/TpsCard";

const CLUSTER_STATS_TIMEOUT = 10000;

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
  const { setActive } = useStatsProvider();
  const { cluster } = useCluster();

  React.useEffect(() => {
    setActive(true);
    return () => setActive(false);
  }, [setActive, cluster]);

  if (
    performanceInfo.status !== ClusterStatsStatus.Ready ||
    dashboardInfo.status !== ClusterStatsStatus.Ready
  ) {
    const error =
      performanceInfo.status === ClusterStatsStatus.Error ||
      dashboardInfo.status === ClusterStatsStatus.Error;
    return <StatsNotReady error={error} />;
  }

  const { avgSlotTime_1h, avgSlotTime_1min, epochInfo } = dashboardInfo;
  const hourlySlotTime = Math.round(1000 * avgSlotTime_1h);
  const averageSlotTime = Math.round(1000 * avgSlotTime_1min) + "ms";
  const { slotIndex, slotsInEpoch } = epochInfo;
  const currentEpoch = epochInfo.epoch.toString();
  const epochProgress = ((100 * slotIndex) / slotsInEpoch).toFixed(1) + "%";
  const epochTimeRemaining = slotsToHumanString(
    slotsInEpoch - slotIndex,
    hourlySlotTime
  );
  const { blockHeight, absoluteSlot } = epochInfo;

  return (
    <TableCardBody>
      <tr>
        <td className="w-100">Slot</td>
        <td className="text-lg-right text-monospace">
          <Slot slot={absoluteSlot} />
        </td>
      </tr>
      {blockHeight !== undefined && (
        <tr>
          <td className="w-100">Block height</td>
          <td className="text-lg-right text-monospace">
            <Slot slot={blockHeight} />
          </td>
        </tr>
      )}
      <tr>
        <td className="w-100">Slot time</td>
        <td className="text-lg-right text-monospace">{averageSlotTime}</td>
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

export function StatsNotReady({ error }: { error: boolean }) {
  const { setTimedOut, retry, active } = useStatsProvider();
  const { cluster } = useCluster();

  React.useEffect(() => {
    let timedOut = 0;
    if (!error) {
      timedOut = setTimeout(setTimedOut, CLUSTER_STATS_TIMEOUT);
    }
    return () => {
      if (timedOut) {
        clearTimeout(timedOut);
      }
    };
  }, [setTimedOut, cluster, error]);

  if (error || !active) {
    return (
      <div className="card-body text-center">
        There was a problem loading cluster stats.{" "}
        <button
          className="btn btn-white btn-sm"
          onClick={() => {
            retry();
          }}
        >
          <span className="fe fe-refresh-cw mr-2"></span>
          Try Again
        </button>
      </div>
    );
  }

  return (
    <div className="card-body text-center">
      <span className="spinner-grow spinner-grow-sm mr-2"></span>
      Loading
    </div>
  );
}
