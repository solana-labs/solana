import React, { Component } from 'react';
import Chart from 'react-apexcharts';
import NumberFormat from 'react-number-format'
import { Row, Col, CardBody, Card } from 'reactstrap';


export default class PerformanceHistory extends Component {

  constructor(props) {
    super(props)
    // Initialize an empty data state
    this.state = {

          RPCdata: {
            perfHistory: {series_s:[{ name: 'TPS', data: [] }],series_m: [{ name: 'TPS', data: [] }],series_l: [{ name: 'TPS', data: [] }]},
            skipRate: {stakeWeightedSkipRate: 0, skipRate: 0},
            stakeWarmup: {msUntilWarmup: 0, epochsUntilWarmup: 0, warmedUpStakePercentage: 0}
          },

         options_s: {
          colors: ['#ffa63f'],
          chart: {
            sparkline: {
              enabled: true,
            },
            type: 'bar',
          },

          fill: {
            colors: ['#2a2c2f','#ffa63f'],
            type: 'gradient',
            gradient: {
              gradientToColors: ['#ffa63f'],
              type: "vertical",
              opacityFrom: 0.7,
              opacityTo: 0.9,
              stops: [0, 90, 100],
              colorStops: [
                {
                  offset: 0,
                  color: '#ffa63f',
                  opacity: 1
                },
                {
                  offset: 100,
                  color: '#2a2c2f',
                  opacity: 1
                }
              ]
            }
          },

          plotOptions: {
            bar: {
              horizontal: false,
              columnWidth: '90%',
              endingShape: 'rounded'
            },
          },
          tooltip: {
            followCursor: false,
            theme: 'dark',
            y: {
              formatter: function (val) {
                return (val/15).toFixed(0)
              }
            },
            x: {
              formatter: function(value) {
                return (30-(value*15)/60).toFixed(0) + " min ago"
              }
            },
          }
        },

      options_m: {
        colors: ['#ffa63f'],
        chart: {
          sparkline: {
            enabled: true,
          },
          type: 'bar',
        },

        fill: {
          colors: ['#2a2c2f','#ffa63f'],
          type: 'gradient',
          gradient: {
            gradientToColors: ['#ffa63f'],
            type: "vertical",
            opacityFrom: 0.7,
            opacityTo: 0.9,
            stops: [0, 90, 100],
            colorStops: [
              {
                offset: 0,
                color: '#ffa63f',
                opacity: 1
              },
              {
                offset: 100,
                color: '#2a2c2f',
                opacity: 1
              }
            ]
          }
        },

        plotOptions: {
          bar: {
            horizontal: false,
            columnWidth: '90%',
            endingShape: 'rounded'
          },
        },
        tooltip: {
          followCursor: false,
          theme: 'dark',
          y: {
            formatter: function (val) {
              return (val/60).toFixed(0)
            }
          },
          x: {
            formatter: function(value) {
              return (120-(value*60)/60).toFixed(0) + " min ago"
            }
          },
        }
      },

      options_l: {
        colors: ['#ffa63f'],
        chart: {
          sparkline: {
            enabled: true,
          },
          type: 'bar',
        },

        fill: {
          colors: ['#2a2c2f','#ffa63f'],
          type: 'gradient',
          gradient: {
            gradientToColors: ['#ffa63f'],
            type: "vertical",
            opacityFrom: 0.7,
            opacityTo: 0.9,
            stops: [0, 90, 100],
            colorStops: [
              {
                offset: 0,
                color: '#ffa63f',
                opacity: 1
              },
              {
                offset: 100,
                color: '#2a2c2f',
                opacity: 1
              }
            ]
          }
        },

        plotOptions: {
          bar: {
            horizontal: false,
            columnWidth: '90%',
            endingShape: 'rounded'
          },
        },
        tooltip: {
          followCursor: false,
          theme: 'dark',
          y: {
            formatter: function (val) {
              return (val/180).toFixed(0)
            }
          },
          x: {
            formatter: function(value) {
              return (360-(value*180)/60).toFixed(0) + " min ago"
            }
          },
        }
      },

        showPerfHistory_short: true,
        showPerfHistory_mid: false,
        showPerfHistory_long: false,
    }
  }

  componentDidMount() {

    const {socket, event} = this.props
    socket.on('performanceInfo', RPCdata => this.handleRPCdata(RPCdata))

  }

  handleRPCdata (data) {

    var RPCdata = {}

    // TPS & Total tx count
    RPCdata.avgTPS = data.avgTPS
    RPCdata.totalTransactionCount = data.totalTransactionCount

    let series_s = [{ name: 'TPS', data: [] }]
    series_s[0].data = data.perfHistory.s

    let series_m = [{ name: 'TPS', data: [] }]
    series_m[0].data = data.perfHistory.m

    let series_l = [{ name: 'TPS', data: [] }]
    series_l[0].data = data.perfHistory.l

    RPCdata.perfHistory = {series_s, series_m, series_l}

    this.setState({RPCdata})

  }

  // toggle different states
  showShortHistory() {
    this.setState({ showPerfHistory_short: true, showPerfHistory_mid: false, showPerfHistory_long: false })
  }
  showMidHistory() {
    this.setState({ showPerfHistory_short: false, showPerfHistory_mid: true, showPerfHistory_long: false })
  }
  showLongHistory() {
    this.setState({ showPerfHistory_short: false, showPerfHistory_mid: false, showPerfHistory_long: true })
  }


  // format numbers as human readable short strings
  nFormatter(num, digits) {
    var si = [
      { value: 1, symbol: "" },
      { value: 1E3, symbol: "k" },
      { value: 1E6, symbol: "M" },
    ];
    var rx = /\.0+$|(\.[0-9]*[1-9])0+$/;
    var i;
    for (i = si.length - 1; i > 0; i--) {
      if (num >= si[i].value) {
        break;
      }
    }
    return (num / si[i].value).toFixed(digits).replace(rx, "$1") + si[i].symbol;
  }


  render() {

    // get max values for our y-axis
    const series_s_max = Math.max(...this.state.RPCdata.perfHistory.series_s[0].data)
    const series_m_max = Math.max(...this.state.RPCdata.perfHistory.series_m[0].data)
    const series_l_max = Math.max(...this.state.RPCdata.perfHistory.series_l[0].data)

    return(

      <div className="container pb-4">
        <Card className="grey-card pl-2">
          <div className="card-indicator bg-orange"></div>
          <Row>
            <Col sm="12" md="4">
              <Row>

                {/* Current TPS */}
                <Col md="12">
                  <Card className="card-box border-0 text-light">
                    <CardBody className="pb-0">
                      <div className="align-box-row align-items-start">
                        <div className="font-weight-bold">
                          <small className="ghostwhite d-block mb-1 text-uppercase">
                            Current TPS
                          </small>
                          <span className="font-size-xxl mt-1 orange"><NumberFormat className="" value={this.state.RPCdata.avgTPS} displayType={'text'} thousandSeparator={true} decimalScale={0}/></span>
                        </div>

                      </div>
                    </CardBody>
                  </Card>
                </Col>

                {/* Total Transactions */}
                <Col md="12">
                  <Card className="card-box border-0 text-light">
                    <CardBody>
                      <div className="align-box-row align-items-start justify-content-between">
                        <div className="font-weight-bold">
                          <small className="ghostwhite d-block mb-1 text-uppercase">
                            Total Transactions
                          </small>
                          <span className="font-size-xxl mt-1 orange"><NumberFormat className="" value={this.state.RPCdata.totalTransactionCount} displayType={'text'} thousandSeparator={true} decimalScale={0}/></span>
                        </div>

                      </div>
                    </CardBody>
                  </Card>
                </Col>

              </Row>
            </Col>

            {/* Bar Chart */}
            <Col sm="12" md="8" className="">
              <Card id="perf-history" className="card-box border-0 text-light">
                <CardBody className="pb-0 w-100">
                  <div className="align-box-row align-items-start justify-content-between">
                    <div className="font-weight-bold w-100">


                      <div className="d-flex justify-content-between">

                        <div>
                          <small className="ghostwhite mb-1 text-uppercase">
                            TPS
                            {this.state.showPerfHistory_short && <span> - 15 sec Ø</span>}
                            {this.state.showPerfHistory_mid && <span> - 1 min Ø</span>}
                            {this.state.showPerfHistory_long && <span> - 3 min Ø</span>}
                          </small>
                        </div>

                        <div className="font-size-sm">

                          <div>
                            <button onClick={() => this.showShortHistory()} className={this.state.showPerfHistory_short ? 'btn-active btn perf-history-btn mr-2' : 'btn perf-history-btn mr-2'}>30m</button>
                            <button onClick={() => this.showMidHistory()} className={this.state.showPerfHistory_mid ? 'btn-active btn perf-history-btn mr-2' : 'btn perf-history-btn mr-2'}>2h</button>
                            <button onClick={() => this.showLongHistory()} className={this.state.showPerfHistory_long ? 'btn-active btn perf-history-btn' : 'btn perf-history-btn'}>6h</button>
                          </div>
                        </div>


                      </div>


                      <div className="mt-2 d-flex justify-content-end flex-row w-100">

                        {this.state.showPerfHistory_short &&
                        <div className="position-relative y-axis">
                          <div className="max-tick">
                            <small>{this.nFormatter(series_s_max/15)}</small>
                          </div>
                          <div className="middle-tick">
                            <small>{this.nFormatter((series_s_max/2)/15)}</small>
                          </div>
                        </div>
                        }

                        {this.state.showPerfHistory_mid &&
                        <div className="position-relative y-axis">
                          <div className="max-tick">
                            <small>{this.nFormatter(series_m_max/60)}</small>
                          </div>
                          <div className="middle-tick">
                            <small>{this.nFormatter((series_m_max/2)/60)}</small>
                          </div>
                        </div>
                        }

                        {this.state.showPerfHistory_long &&
                        <div className="position-relative y-axis">
                          <div className="max-tick">
                            <small>{this.nFormatter(series_l_max/180)}</small>
                          </div>
                          <div className="middle-tick">
                            <small>{this.nFormatter((series_l_max/2)/180)}</small>
                          </div>
                        </div>
                        }




                        <div className="w-100">
                          {this.state.showPerfHistory_short &&
                          <Chart
                            options={this.state.options_s}
                            //series={this.state.series}
                            series={this.state.RPCdata.perfHistory.series_s}
                            type="bar"
                            height={120}
                          />
                          }
                          {this.state.showPerfHistory_mid &&
                          <Chart
                            options={this.state.options_m}
                            //series={this.state.series}
                            series={this.state.RPCdata.perfHistory.series_m}
                            type="bar"
                            height={120}
                          />
                          }
                          {this.state.showPerfHistory_long &&
                          <Chart
                            options={this.state.options_l}
                            //series={this.state.series}
                            series={this.state.RPCdata.perfHistory.series_l}
                            type="bar"
                            height={120}
                          />
                          }
                        </div>

                      </div>

                    </div>

                  </div>
                </CardBody>
              </Card>

            </Col>

          </Row>
        </Card>
      </div>




  )}
}


