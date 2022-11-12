import { useState, useEffect } from "react";
import axios from "axios";
import { ClusterStatus, useCluster } from "providers/cluster";

const PING_PERIOD_IN_MS: number = 1000;

const container = {
  display: "flex",
  width: "100%",
  justifyContent: "center",
  backgroundColor: "rgba(8, 230, 189, 0.941)",
  color: "black",
  fontWeight: "bold" as "bold",
};
function NetworkStatusNotifier() {
  const healthyStatus: string = "ok";
  const [currentStatus, setCurrentStatus] = useState<string>(healthyStatus);
  const [hasDownTime, setHasDownTime] = useState<boolean>(false);

  const { url, name, status } = useCluster();

  useEffect(() => {
    let timer = setInterval(async () => {
      if (status === ClusterStatus.Connected) {
        const res = await makeHealthCall(url);
        let statusDesc: string = "";

        if (res.result) {
          statusDesc = res.result;
        }
        if (res.error) {
          statusDesc = res.error.message;
        }

        let hasHealthStatusChanged = statusDesc !== currentStatus;

        if (hasHealthStatusChanged) {
          setHasDownTime(statusDesc !== healthyStatus);
          setCurrentStatus(statusDesc);
        }
      }
    }, PING_PERIOD_IN_MS);

    return () => {
      clearTimeout(timer);
    };
  });

  return (
    <div style={container}>
      {hasDownTime && (
        <div>
          {name} cluster may be down. Downtime Detail: {currentStatus}
        </div>
      )}
    </div>
  );
}

export default NetworkStatusNotifier;

export async function makeHealthCall(url: string): Promise<any> {
  try {
    const config = {
      headers: {
        "Content-Type": "application/json",
      },
    };

    const data = {
      jsonrpc: "2.0",
      id: 1,
      method: "getHealth",
    };

    const response = await axios.post(url, data, config);
    return response.data;
  } catch {
    //ignore error in production
  }
}
