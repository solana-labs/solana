import React, { Component } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { Row, Col, CardBody, Card } from "reactstrap";
import HashFormat from "./HashFormat";
import solanaLogo from "../../img/solanabeach/logo.png";
import questionmarkLogo from "../../img/solanabeach/questionmark.png";

import {
  ZoomableGroup,
  ComposableMap,
  Geographies,
  Geography,
  Annotation,
  Marker,
} from "react-simple-maps";

const geoUrl =
  "https://raw.githubusercontent.com/zcreativelabs/react-simple-maps/master/topojson-maps/world-110m.json";

function renderAvatar(validator, leader) {
  if (validator == null) return;

  let className = "avatar-icon rounded-circle";
  if (leader) {
    className += " avatar-icon-leader";
  }
  if (validator.moniker) {
    return <img alt={""} className={className} src={validator.pictureURL} />;
  }

  switch (validator.nodePubkey) {
    case "CakcnaRDHka2gXyfbEd2d3xsvkJkqsLw2akB3zsN1D2S":
      return <img alt={""} className={className} src={solanaLogo} />;
    case "7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2":
      return <img alt={""} className={className} src={solanaLogo} />;
    case "GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ":
      return <img alt={""} className={className} src={solanaLogo} />;
    case "DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ":
      return <img alt={""} className={className} src={solanaLogo} />;
    case "5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on":
      return <img alt={""} className={className} src={solanaLogo} />;
    default:
      return <img alt={""} className={className} src={questionmarkLogo} />;
  }
}

function renderMoniker(validator) {
  if (validator == null) return;

  if (validator.delinquent) {
    if (validator.moniker) {
      return (
        <a
          href={validator.website}
          className="font-weight-500 coralred"
          title="..."
        >
          {validator.moniker}
        </a>
      );
    }
    switch (validator.nodePubkey) {
      case "CakcnaRDHka2gXyfbEd2d3xsvkJkqsLw2akB3zsN1D2S":
        return (
          <div className="font-weight-500 coralred">Solana Boot Node 1</div>
        );
      case "7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2":
        return (
          <div className="font-weight-500 coralred">Solana Boot Node 2</div>
        );
      case "GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ":
        return (
          <div className="font-weight-500 coralred">Solana Boot Node 3</div>
        );
      case "DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ":
        return (
          <div className="font-weight-500 coralred">Solana Boot Node 4</div>
        );
      case "5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on":
        return <div className="font-weight-500 coralred">Solana Boot Node</div>;
      default:
        return (
          <div className="font-weight-500 coralred">
            <HashFormat hash={validator.nodePubkey} />
          </div>
        );
    }
  }

  if (validator.moniker) {
    return (
      <a href={validator.website} className="font-weight-500 " title="...">
        {validator.moniker}
      </a>
    );
  }
  switch (validator.nodePubkey) {
    case "CakcnaRDHka2gXyfbEd2d3xsvkJkqsLw2akB3zsN1D2S":
      return <div className="font-weight-500 ">Solana Boot Node 1</div>;
    case "7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2":
      return <div className="font-weight-500 ">Solana Boot Node 2</div>;
    case "GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ":
      return <div className="font-weight-500 ">Solana Boot Node 3</div>;
    case "DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ":
      return <div className="font-weight-500 ">Solana Boot Node 4</div>;
    case "5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on":
      return <div className="font-weight-500 ">Solana Boot Node</div>;
    default:
      return (
        <div className="font-weight-500 ">
          <HashFormat hash={validator.nodePubkey} />
        </div>
      );
  }
}

function calcMarkerColor(nodeCount) {
  if (nodeCount === 1) return "palegreen";
  if (nodeCount < 5) return "yellowgreen";
  if (nodeCount < 10) return "limegreen";
  return "lime";
}

function calcMarkerRadius(nodeCount) {
  if (nodeCount === 1) return 10;
  if (nodeCount < 5) return 11;
  if (nodeCount < 10) return 12;
  return 14;
}

function renderMapLocationLabel(currentLeader) {
  if (currentLeader == null) return;

  if (currentLeader.location) {
    if (currentLeader.location.country && currentLeader.location.city) {
      return (
        currentLeader.location.country + ", " + currentLeader.location.city
      );
    }
    return currentLeader.location.country;
  }
  return "";
}

export default class LeaderWorldMap extends Component {
  constructor(props) {
    super(props);

    this.state = {
      root: 0,
      mapInitialized: false,
      currentLeader: { location: { country: "", city: "" } },
      servedSlots: [],
      nextLeaders: [],
      markers: [
        { coordinates: [100, 100], nodeCount: 0, leader: false, pubkeys: [] },
      ],
    };
  }

  componentWillUnmount() {
    if (this.socket) {
      this.socket.disconnect();
    }
  }

  componentDidMount() {
    const { socket } = this.props;
    socket.on("validatorInfo", (RPCdata) => this.handleRPCdata(RPCdata));
    socket.on("rootNotification", (data) => this.handleNewRoot(data));
  }

  handleNewRoot(data) {
    if (data.currentLeader) {
      let markers = this.state.markers;

      // abort if there is no new leader
      if (
        data.currentLeader.nodePubkey === this.state.currentLeader.nodePubkey
      ) {
        this.setState({ servedSlots: data.servedSlots });
        return;
      }

      // mark the new leader on the map
      for (let i = 0; i < markers.length; i++) {
        for (let j = 0; j < markers[i].pubkeys.length; j++) {
          if (markers[i].pubkeys[j] === data.currentLeader.nodePubkey) {
            markers[i].leader = true;
            break;
          } else {
            markers[i].leader = false;
          }
        }
      }
      // Place markers with high node counts on top
      markers.sort((a, b) => (a.nodeCount > b.nodeCount ? 1 : -1));

      this.setState({
        markers: markers,
        currentLeader: data.currentLeader,
        servedSlots: data.servedSlots,
        nextLeaders: data.nextLeaders,
      });
    }
  }

  handleRPCdata(data) {
    if (this.state.mapInitialized) {
      return;
    }

    let markers = [];

    for (const key in data.validatorSet) {
      // we are not able to get location info for every validator
      if (!data.validatorSet[key].location) {
        continue;
      }

      // a marker holds location coordinates and the pubkeys of all validators residing in that location
      let marker = { pubkeys: [] };
      let markerIsSet = false;

      for (let i = 0; i < markers.length; i++) {
        // accumulate similar coordinates under one entry, increase node count
        // modify epsilon (approxeq) to increase/decrease location grouping precision
        if (
          this.approxeq(
            markers[i].coordinates[0],
            data.validatorSet[key].location.ll[0]
          ) &&
          this.approxeq(
            markers[i].coordinates[1],
            data.validatorSet[key].location.ll[1]
          )
        ) {
          markers[i].nodeCount = markers[i].nodeCount + 1;
          markers[i].pubkeys.push(data.validatorSet[key].nodePubkey);
          markerIsSet = true;
          break;
        }
      }

      // create a new marker if there is no existing one in the current location
      if (!markerIsSet) {
        marker.nodeCount = 1;
        marker.country = data.validatorSet[key].location.country;
        marker.coordinates = data.validatorSet[key].location.ll;
        marker.pubkeys.push(data.validatorSet[key].nodePubkey);
        markers.push(marker);
      }
    }

    // reverse all ll coordinates to display correctly with simple maps
    for (let i = 0; i < markers.length; i++) {
      markers[i].coordinates.reverse();
    }

    this.setState({ markers, mapInitialized: true });
  }

  // helper to group nearby coordinates
  approxeq = function (v1, v2, epsilon) {
    if (epsilon == null) {
      epsilon = 1;
    }
    return Math.abs(v1 - v2) < epsilon;
  };

  render() {
    let currentLeader = this.state.currentLeader;
    let servedSlots = this.state.servedSlots;
    let nextLeaders = this.state.nextLeaders;
    let markers = this.state.markers;

    return (
      <Row style={{ minHeight: "200px" }}>
        <Col md="5" lg="5">
          <CardBody className="mb-xl-3 mt-xl-3">
            <div className="d-flex flex-row align-items-start">
              <div className="mr-3">
                <div className="text-center text-success font-size-xl d-50 rounded-circle">
                  {renderAvatar(currentLeader, true)}
                </div>
              </div>
              <div>
                <div className="font-weight-bold">
                  <small className=" d-block mb-1 text-uppercase">
                    Current Leader
                  </small>
                  <span className="mt-2">{renderMoniker(currentLeader)}</span>
                </div>
                <div className="mt-2">
                  {servedSlots.map((item, i) => {
                    return item === true ? (
                      <FontAwesomeIcon
                        icon={["fas", "square"]}
                        className="palegreen pr-1"
                      />
                    ) : (
                      <FontAwesomeIcon
                        icon={["fas", "square"]}
                        className="border-grey pr-1"
                      />
                    );
                  })}
                </div>
              </div>
            </div>
          </CardBody>

          <CardBody className="pt-0 mb-2">
            <small className=" d-block mb-3 text-uppercase">Next leaders</small>
            <div className="ml-auto d-flex flex-row align-items-center align-content-center">
              <div className="avatar-wrapper-overlap d-flex flex-row">
                {nextLeaders.map((item, i) => {
                  return (
                    <div className="text-center text-success font-size-xl d-50 rounded-circle">
                      {renderAvatar(item)}
                    </div>
                  );
                })}
              </div>
            </div>
          </CardBody>
        </Col>

        <Col md="7" lg="7">
          <Card className="card-box border-0 pb-0 mb-0">
            <CardBody style={{ paddingTop: "1.25rem", padding: "0" }}>
              <ComposableMap
                width={1100}
                height={450}
                projection="geoMercator"
                projectionConfig={{
                  yOffset: 150,
                  rotate: [0, 0, 0],
                  scale: 150,
                }}
              >
                <ZoomableGroup zoom={1.175} center={[10, 25]}>
                  <Geographies geography={geoUrl}>
                    {({ geographies }) =>
                      geographies.map((geo) => (
                        <Geography
                          key={geo.rsmKey}
                          geography={geo}
                          fill="#414046"
                          stroke="#828282"
                        />
                      ))
                    }
                  </Geographies>

                  <Annotation
                    subject={[-160.3522, -28.8566]}
                    dx={0}
                    dy={0}
                    connectorProps={{
                      stroke: "#FF5533",
                      strokeWidth: 0,
                      strokeLinecap: "round",
                    }}
                  >
                    <text
                      x="-8"
                      textAnchor="start"
                      alignmentBaseline="middle"
                      fill="black"
                      style={{ fontSize: "1.2rem", fontWeight: "500" }}
                    >
                      {renderMapLocationLabel(currentLeader)}
                    </text>
                  </Annotation>

                  {markers.map(
                    ({ leader, nodeCount, country, coordinates }) => {
                      return leader === true ? (
                        <Marker
                          coordinates={coordinates}
                          style={{ zIndex: "1000" }}
                        >
                          <defs>
                            <linearGradient id="Gradient1">
                              <stop className="stop1" offset="0%" />
                              <stop className="stop2" offset="50%" />
                              <stop className="stop3" offset="100%" />
                            </linearGradient>
                            <linearGradient
                              id="Gradient2"
                              x1="0"
                              x2="0"
                              y1="0"
                              y2="1"
                            >
                              <stop offset="0%" stop-color="#e4964e" />
                              <stop offset="50%" stop-color="#e66f62" />
                              <stop offset="100%" stop-color="#e5586d" />
                            </linearGradient>
                          </defs>

                          <circle
                            r={calcMarkerRadius(nodeCount)}
                            fill={calcMarkerColor(nodeCount)}
                            stroke="#000000"
                            strokeWidth={1}
                          />
                          <text
                            textAnchor="middle"
                            y={5}
                            style={{ fontFamily: "system-ui", fill: "black" }}
                          >
                            {nodeCount}
                          </text>

                          <g transform="translate(-18, -58)">
                            <path
                              stroke="black"
                              strokeWidth={1}
                              fill="url(#Gradient2)"
                              id="svg_1"
                              d="m18.586387,0.332172c-10.248667,0 -18.586387,8.311892 -18.586387,18.528714c0,15.222386 16.736357,30.085726 17.449082,30.711171c0.324896,0.285129 0.731017,0.427945 1.137305,0.427945c0.406288,0 0.812409,-0.142648 1.137473,-0.427945c0.712389,-0.625277 17.448914,-15.488618 17.448914,-30.711171c0,-10.216822 -8.337719,-18.528714 -18.586387,-18.528714zm0,13.327155c2.846871,0 5.162932,2.333377 5.162932,5.201559c0,2.868182 -2.316061,5.201559 -5.162932,5.201559c-2.846871,0 -5.162932,-2.333377 -5.162932,-5.201559c0,-2.868182 2.316061,-5.201559 5.162932,-5.201559z"
                            />
                          </g>
                        </Marker>
                      ) : (
                        <Marker coordinates={coordinates}>
                          <circle
                            r={calcMarkerRadius(nodeCount)}
                            fill={calcMarkerColor(nodeCount)}
                            stroke="#000000"
                            strokeWidth={1}
                          />
                          <text
                            textAnchor="middle"
                            y={5}
                            style={{ fontFamily: "system-ui", fill: "black" }}
                          >
                            {nodeCount}
                          </text>
                        </Marker>
                      );
                    }
                  )}
                </ZoomableGroup>
              </ComposableMap>
            </CardBody>
          </Card>
        </Col>
      </Row>
    );
  }
}
