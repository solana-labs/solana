import React, { useState, useEffect } from "react";

const STATUS_API_ENDPOINT: string =
  "https://status.solana.com/api/v2/status.json";

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
  const healthyStatus = "All Systems Operational";
  const [currentDownStatus, setCurrentErrorState] =
    useState<string>(healthyStatus);
  const [hasDownTime, setHasDownTime] = useState<boolean>(false);

  useEffect(() => {
    let timer = setInterval(async () => {
      const res = await makeGetRequest(STATUS_API_ENDPOINT);
      const statusDesc = res.status.description;
      setHasDownTime(statusDesc !== healthyStatus);
      setCurrentErrorState(statusDesc);
    }, PING_PERIOD_IN_MS);

    return () => {
      clearTimeout(timer);
    };
  }, []);

  return (
    <div style={container}>
      {hasDownTime && (
        <div>
          Solana network may be down. Downtime Detail:
          {currentDownStatus}
        </div>
      )}
    </div>
  );
}

export default NetworkStatusNotifier;

async function makeGetRequest(url: string): Promise<any> {
  const res = await fetch(url);
  return await res.json();
}
