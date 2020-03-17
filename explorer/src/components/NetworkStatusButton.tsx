import React from "react";
import { useNetwork, NetworkStatus } from "../providers/network";

function NetworkStatusButton() {
  const { status, url } = useNetwork();

  switch (status) {
    case NetworkStatus.Connected:
      return <span className="btn btn-white lift">{url}</span>;

    case NetworkStatus.Connecting:
      return (
        <span className="btn btn-warning lift">
          {"Connecting "}
          <span
            className="spinner-grow spinner-grow-sm text-dark"
            role="status"
            aria-hidden="true"
          ></span>
        </span>
      );

    case NetworkStatus.Failure:
      return <span className="btn btn-danger lift">Disconnected</span>;
  }
}

export default NetworkStatusButton;
