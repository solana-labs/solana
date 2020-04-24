import React from "react";
import {
  useCluster,
  useClusterDispatch,
  updateCluster,
  ClusterStatus,
  clusterUrl,
  clusterName,
  CLUSTERS,
  Cluster
} from "../providers/cluster";
import { assertUnreachable } from "../utils";
import Overlay from "./Overlay";

type Props = {
  show: boolean;
  onClose: () => void;
};

function ClusterModal({ show, onClose }: Props) {
  return (
    <>
      <div
        className={`modal fade fixed-right${show ? " show" : ""}`}
        onClick={onClose}
      >
        <div className="modal-dialog modal-dialog-vertical">
          <div className="modal-content">
            <div className="modal-body" onClick={e => e.stopPropagation()}>
              <span className="close" onClick={onClose}>
                &times;
              </span>

              <h2 className="text-center mb-4 mt-4">Choose a Cluster</h2>

              <ClusterToggle />
            </div>
          </div>
        </div>
      </div>

      <Overlay show={show} />
    </>
  );
}

type InputProps = { activeSuffix: string; active: boolean };
function CustomClusterInput({ activeSuffix, active }: InputProps) {
  const { customUrl } = useCluster();
  const dispatch = useClusterDispatch();
  const [editing, setEditing] = React.useState(false);

  const customClass = (prefix: string) =>
    active ? `${prefix}-${activeSuffix}` : "";

  const inputTextClass = editing ? "" : "text-muted";
  return (
    <div
      className="btn input-group input-group-merge p-0"
      onClick={() =>
        !active && updateCluster(dispatch, Cluster.Custom, customUrl)
      }
    >
      <input
        type="text"
        defaultValue={customUrl}
        className={`form-control form-control-prepended ${inputTextClass} ${customClass(
          "border"
        )}`}
        onFocus={() => setEditing(true)}
        onBlur={() => setEditing(false)}
        onInput={e =>
          updateCluster(dispatch, Cluster.Custom, e.currentTarget.value)
        }
      />
      <div className="input-group-prepend">
        <div className={`input-group-text pr-0 ${customClass("border")}`}>
          <span className={customClass("text") || "text-dark"}>Custom:</span>
        </div>
      </div>
    </div>
  );
}

function ClusterToggle() {
  const { status, cluster, customUrl } = useCluster();
  const dispatch = useClusterDispatch();

  let activeSuffix = "";
  switch (status) {
    case ClusterStatus.Connected:
      activeSuffix = "success";
      break;
    case ClusterStatus.Connecting:
      activeSuffix = "warning";
      break;
    case ClusterStatus.Failure:
      activeSuffix = "danger";
      break;
    default:
      assertUnreachable(status);
  }

  return (
    <div className="btn-group-toggle d-flex flex-wrap mb-4">
      {CLUSTERS.map((net, index) => {
        const active = net === cluster;
        if (net === Cluster.Custom)
          return (
            <CustomClusterInput
              key={index}
              activeSuffix={activeSuffix}
              active={active}
            />
          );

        const btnClass = active
          ? `border-${activeSuffix} text-${activeSuffix}`
          : "btn-white text-dark";

        return (
          <label
            key={index}
            className={`btn text-left col-12 mb-3 ${btnClass}`}
          >
            <input
              type="radio"
              checked={active}
              onChange={() => updateCluster(dispatch, net, customUrl)}
            />
            {`${clusterName(net)}: `}
            <span className="text-muted d-inline-block">
              {clusterUrl(net, customUrl)}
            </span>
          </label>
        );
      })}
    </div>
  );
}

export default ClusterModal;
