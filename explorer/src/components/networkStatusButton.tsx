import React from "react";
import { useNetwork, NetworkStatus } from "../providers/network";

function NetworkStatusButton() {
  const { status, url } = useNetwork();

  switch (status) {
    case NetworkStatus.Connected:
      return (
        <a href="#networkModal" className="btn btn-primary lift">
          {url}
        </a>
      );

    case NetworkStatus.Connecting:
      return (
        <a href="#networkModal" className="btn btn-warning lift">
          {"Connecting "}
          <span
            className="spinner-grow spinner-grow-sm text-dark"
            role="status"
            aria-hidden="true"
          ></span>
        </a>
      );

    case NetworkStatus.Failure:
      return (
        <a href="#networkModal" className="btn btn-danger lift">
          Disconnected
        </a>
      );
  }
}

export default NetworkStatusButton;
