import React, { ChangeEvent } from "react";
import { Link, useHistory, useLocation } from "react-router-dom";
import { useDebounceCallback } from "@react-hook/debounce";
import { Location } from "history";
import {
  useCluster,
  ClusterStatus,
  clusterName,
  clusterSlug,
  CLUSTERS,
  Cluster,
  useClusterModal,
  useUpdateCustomUrl,
} from "providers/cluster";
import { assertUnreachable, localStorageIsAvailable } from "../utils";
import { Overlay } from "./common/Overlay";
import { useQuery } from "utils/url";

export function ClusterModal() {
  const [show, setShow] = useClusterModal();
  const onClose = () => setShow(false);
  const showDeveloperSettings = localStorageIsAvailable();
  const enableCustomUrl =
    showDeveloperSettings && localStorage.getItem("enableCustomUrl") !== null;
  const onToggleCustomUrlFeature = (e: ChangeEvent<HTMLInputElement>) => {
    if (e.target.checked) {
      localStorage.setItem("enableCustomUrl", "");
    } else {
      localStorage.removeItem("enableCustomUrl");
    }
  };

  return (
    <>
      <div className={`offcanvas offcanvas-end${show ? " show" : ""}`}>
        <div className="modal-body" onClick={(e) => e.stopPropagation()}>
          <span className="c-pointer" onClick={onClose}>
            &times;
          </span>

          <h2 className="text-center mb-4 mt-4">Choose a Cluster</h2>
          <ClusterToggle />

          {showDeveloperSettings && (
            <>
              <hr />

              <h2 className="text-center mb-4 mt-4">Developer Settings</h2>
              <div className="d-flex justify-content-between">
                <span className="me-3">Enable custom url param</span>
                <div className="form-check form-switch">
                  <input
                    type="checkbox"
                    defaultChecked={enableCustomUrl}
                    className="form-check-input"
                    id="cardToggle"
                    onChange={onToggleCustomUrlFeature}
                  />
                  <label
                    className="form-check-label"
                    htmlFor="cardToggle"
                  ></label>
                </div>
              </div>
              <p className="text-muted font-size-sm mt-3">
                Enable this setting to easily connect to a custom cluster via
                the "customUrl" url param.
              </p>
            </>
          )}
        </div>
      </div>

      <div onClick={onClose}>
        <Overlay show={show} />
      </div>
    </>
  );
}

type InputProps = { activeSuffix: string; active: boolean };
function CustomClusterInput({ activeSuffix, active }: InputProps) {
  const { customUrl } = useCluster();
  const updateCustomUrl = useUpdateCustomUrl();
  const [editing, setEditing] = React.useState(false);
  const query = useQuery();
  const history = useHistory();
  const location = useLocation();

  const btnClass = active
    ? `border-${activeSuffix} text-${activeSuffix}`
    : "btn-white";

  const clusterLocation = (location: Location) => {
    query.set("cluster", "custom");
    if (customUrl.length > 0) {
      query.set("customUrl", customUrl);
    }
    return {
      ...location,
      search: query.toString(),
    };
  };

  const onUrlInput = useDebounceCallback((url: string) => {
    updateCustomUrl(url);
    if (url.length > 0) {
      query.set("customUrl", url);
      history.push({ ...location, search: query.toString() });
    }
  }, 500);

  const inputTextClass = editing ? "" : "text-muted";
  return (
    <>
      <Link className={`btn col-12 mb-3 ${btnClass}`} to={clusterLocation}>
        Custom RPC URL
      </Link>
      {active && (
        <input
          type="url"
          defaultValue={customUrl}
          className={`form-control ${inputTextClass}`}
          onFocus={() => setEditing(true)}
          onBlur={() => setEditing(false)}
          onInput={(e) => onUrlInput(e.currentTarget.value)}
        />
      )}
    </>
  );
}

function ClusterToggle() {
  const { status, cluster } = useCluster();

  let activeSuffix = "";
  switch (status) {
    case ClusterStatus.Connected:
      activeSuffix = "primary";
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
          : "btn-white";

        const clusterLocation = (location: Location) => {
          const params = new URLSearchParams(location.search);
          const slug = clusterSlug(net);
          if (slug !== "mainnet-beta") {
            params.set("cluster", slug);
          } else {
            params.delete("cluster");
          }
          return {
            ...location,
            search: params.toString(),
          };
        };

        return (
          <Link
            key={index}
            className={`btn col-12 mb-3 ${btnClass}`}
            to={clusterLocation}
          >
            {clusterName(net)}
          </Link>
        );
      })}
    </div>
  );
}
