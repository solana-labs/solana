import React from "react";
import {
  useCluster,
  ClusterStatus,
  Cluster,
  useClusterModal,
} from "providers/cluster";

export function ClusterStatusBanner() {
  const [, setShow] = useClusterModal();

  return (
    <div className="container d-md-none my-4">
      <div onClick={() => setShow(true)}>
        <Button />
      </div>
    </div>
  );
}

export function ClusterStatusButton() {
  const [, setShow] = useClusterModal();

  return (
    <div onClick={() => setShow(true)}>
      <Button />
    </div>
  );
}

function Button() {
  const { status, cluster, name, customUrl } = useCluster();
  const statusName = cluster !== Cluster.Custom ? `${name}` : `${customUrl}`;

  const btnClasses = (variant: string) => {
    return `btn d-block btn-${variant}`;
  };

  const spinnerClasses = "spinner-grow spinner-grow-sm me-2";

  switch (status) {
    case ClusterStatus.Connected:
      return (
        <span className={btnClasses("primary")}>
          <span className="fe fe-check-circle me-2"></span>
          {statusName}
        </span>
      );

    case ClusterStatus.Connecting:
      return (
        <span className={btnClasses("warning")}>
          <span
            className={spinnerClasses}
            role="status"
            aria-hidden="true"
          ></span>
          {statusName}
        </span>
      );

    case ClusterStatus.Failure:
      return (
        <span className={btnClasses("danger")}>
          <span className="fe fe-alert-circle me-2"></span>
          {statusName}
        </span>
      );
  }
}
