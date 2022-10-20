import { useCluster } from "providers/cluster";
import React, { useState, useEffect } from "react";
import axios from "axios";

const PING_PERIOD_IN_MS: number = 3000;

const container = {
  display: "flex",
  width: "100%",
  justifyContent: "center",
  backgroundColor: "rgba(8, 230, 189, 0.941)",
  color: "black",
  fontWeight: "bold" as "bold",
};
function NetworkStatusNotifier() {
  const healthyStatus = "ok";
  const [currentDownStatus, setCurrentErrorState] =
    useState<string>(healthyStatus);
  const [hasDownTime, setHasDownTime] = useState<boolean>(false);

  const { url, name } = useCluster();

  useEffect(() => {
    let timer = setInterval(async () => {
      const statusDesc = await makeAHealthCheckCall(url);

      if (statusDesc !== currentDownStatus) {
        setHasDownTime(statusDesc !== healthyStatus);
        setCurrentErrorState(statusDesc);
      }
    }, PING_PERIOD_IN_MS);

    return () => {
      clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [url]);

  return (
    <div style={container}>
      {hasDownTime && (
        <div>
          {name} cluster may be down. Downtime Detail: {currentDownStatus}
        </div>
      )}
    </div>
  );
}

export default NetworkStatusNotifier;

const makeAHealthCheckCall = async (url: string): Promise<any> => {
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
    const res = await axios.post(url, data, config);
    return res.data.result;
  } catch (error) {
    //ignore the error for now but Will check for other error handling mechanism in future.
  }
};
