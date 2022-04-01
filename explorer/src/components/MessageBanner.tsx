import React from "react";
import { useCluster, Cluster } from "providers/cluster";
import { displayTimestamp } from "utils/date";

type Announcement = {
  message: string;
  estimate?: string;
  start?: Date;
  end?: Date;
};

const announcements = new Map<Cluster, Announcement>();
// announcements.set(Cluster.Devnet, {
//   message: "Devnet API node is restarting",
//   start: new Date("July 25, 2020 18:00:00 GMT+8:00"),
//   estimate: "2 hours",
// });
// announcements.set(Cluster.MainnetBeta, {
//   message: "Mainnet Beta upgrade in progress. Transactions disabled until epoch 62",
//   start: new Date("August 2, 2020 00:00:00 GMT+0:00"),
//   end: new Date("August 4, 2020 00:00:00 GMT+0:00"),
// });
// announcements.set(Cluster.MainnetBeta, {
//   message:
//     "Mainnet Beta upgrade in progress. Transactions disabled until epoch 62",
// });

export function MessageBanner() {
  const cluster = useCluster().cluster;
  const announcement = announcements.get(cluster);
  if (!announcement) return null;
  const { message, start, end, estimate } = announcement;

  const now = new Date();
  if (end && now > end) return null;
  if (start && now < start) return null;

  let timeframe;
  if (estimate || start || end) {
    timeframe = (
      <div>
        <hr className="text-gray-500 w-100 my-3 opacity-50" />
        {estimate && (
          <h5 className="font-sm text-gray-200">
            <span className="text-uppercase">Estimated Duration: </span>
            {estimate}
          </h5>
        )}
        {start && (
          <h5 className="font-sm text-gray-200">
            <span className="text-uppercase">Started at: </span>
            {displayTimestamp(start.getTime())}
          </h5>
        )}
        {end && (
          <h5 className="font-sm text-gray-200">
            <span className="text-uppercase">End: </span>
            {displayTimestamp(end.getTime())}
          </h5>
        )}
      </div>
    );
  }

  return (
    <div className="bg-info">
      <div className="container">
        <div className="d-flex flex-column align-items-center justify-content-center text-center py-3">
          <h3 className="mb-0 line-height-md">
            <span className="fe fe-alert-circle me-2"></span>
            {message}
          </h3>
          {timeframe}
        </div>
      </div>
    </div>
  );
}
