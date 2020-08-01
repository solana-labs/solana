import React, { Component } from "react";
import { Col, CardBody } from "reactstrap";
import NumberFormat from "react-number-format";

export default class BlockHeight extends Component {
  constructor(props) {
    super(props);

    this.state = {
      data: 0,
    };
  }

  componentWillUnmount() {
    // Disconnect socket when component is unmounted
    if (this.socket) {
      this.socket.disconnect();
    }
  }

  componentDidMount() {
    const { socket, event } = this.props;

    socket.on(event, (data) => this.setState({ data }));
  }

  render() {
    return (
      <Col md="6" lg="4">
        <div className="card-box border-0">
          <CardBody>
            <div className="align-box-row align-items-start">
              <div className="font-weight-bold">
                <small className="ghostwhite d-block mb-1 text-uppercase">
                  Block Height
                </small>
                <span className="font-size-xxl mt-1 palegreen">
                  <NumberFormat
                    className=""
                    value={this.state.data.root}
                    displayType={"text"}
                    thousandSeparator={true}
                    decimalScale={0}
                  />
                </span>
              </div>
            </div>
            <div className="mt-3">
              <span className="ghostwhite font-size-sm">
                Last finalized block
              </span>
            </div>
          </CardBody>
        </div>
      </Col>
    );
  }
}
