import React, { Component, Fragment } from "react";
import io from "socket.io-client";
import NetworkStats from "./NetworkStats";
import PerformanceHistory from "./PerformanceHistory";

export default class LandingPage extends Component {
  constructor(props) {
    super(props);

    // Connect socket before component is mounted

    this.socket = io("https://api.solanabeach.io:8443/mainnet");

    this.socket.on("connect", () => this.requestData());

    this.socket.on("error", (err) => {
      console.log("error", err);
    });
  }

  requestData() {
    this.socket.emit("request_dashboardInfo");
    this.socket.emit("request_validatorInfo");
    this.socket.emit("request_performanceInfo");
  }

  componentWillUnmount() {
    // Disconnect socket when component is unmounted
    if (this.socket) {
      this.socket.disconnect();
    }
  }

  render() {
    return (
      <Fragment>
        <div className="hero-wrapper bg-composed-wrapper withOverflowingBackground">
          {/*<NetworkStats socket={this.socket} location={this.props.location} />*/}
          <PerformanceHistory socket={this.socket} />
        </div>
      </Fragment>
    );
  }
}
