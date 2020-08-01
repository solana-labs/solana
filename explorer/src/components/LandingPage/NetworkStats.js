import React, { Fragment, Component } from 'react';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Row, Col, CardBody, Card, Progress } from 'reactstrap';
import NumberFormat from 'react-number-format'
import BlockHeight from './BlockHeight';
import LeaderWorldMap from './LeaderWorldMap';
import 'moment-duration-format';

const moment = require('moment')
const DROP = 1000000000;

export default class NetworkStats extends Component {

  constructor(props) {
    super(props)

    this.state = {

      RPCdata: {
        stakeWarmup: {msUntilWarmup: 0, epochsUntilWarmup: 0, warmedUpStakePercentage: 0}
      },
    }
  }

  componentDidMount() {

    const {socket, event} = this.props
    socket.on('dashboardInfo', RPCdata => this.handleRPCdata(RPCdata))

  }


  handleRPCdata (data) {

    var RPCdata = {}

    //TODO: find out why backend reports data.epochInfo.slotsInEpoch as 0 from time to time
    // temp fix:
    let slotsInEpoch
    if (data.epochInfo.slotsInEpoch === 0) {slotsInEpoch=420000} else {
      slotsInEpoch = data.epochInfo.slotsInEpoch
    }

    // Block Time & Epoch & Skip Rate
    RPCdata.avgBlockTime_1min = data.avgBlockTime_1min
    RPCdata.avgBlockTime_1h = data.avgBlockTime_1h
    RPCdata.epochPercent = (data.epochInfo.slotIndex/slotsInEpoch)*100
    RPCdata.msUntilNextEpoch = (slotsInEpoch - data.epochInfo.slotIndex)*RPCdata.avgBlockTime_1h*1000
    RPCdata.epoch = data.epochInfo.epoch
    RPCdata.skipRate = data.skipRate

    // Economics
    RPCdata.activatedStake = data.activatedStake/DROP
    RPCdata.totalDelegatedStake = data.totalDelegatedStake/DROP
    RPCdata.delinquentStake = data.delinquentStake/DROP
    RPCdata.totalSupply = data.totalSupply/DROP
    RPCdata.circulatingSupply = data.circulatingSupply/DROP
    RPCdata.stakingYield = data.stakingYield
    RPCdata.realStakingYield = data.stakingYield
    RPCdata.tokenPrice = data.tokenPrice
    RPCdata.dailyPriceChange = data.dailyPriceChange
    RPCdata.dailyVolume = data.dailyVolume
    RPCdata.fullyDilutedMcap = (data.totalSupply/DROP)*data.tokenPrice

    // Stake Warmup Challenge
    RPCdata.stakeWarmup = this.calcStakeWarmupDuration(RPCdata.totalDelegatedStake, RPCdata.activatedStake, RPCdata.msUntilNextEpoch, RPCdata.avgBlockTime_1h)

    this.setState({RPCdata})

  }

  // Calc how long it will take for all stake to warm up
  calcStakeWarmupDuration(totalDelegated, activatedStake, msUntilNextEpoch, blockTime) {

    let res = {msUntilWarmup: 0, epochsUntilWarmup: 0, warmedUpStakePercentage: 100}
    res.warmedUpStakePercentage = activatedStake/totalDelegated*100

    if (activatedStake >= totalDelegated) {return res}

    const epochBlocks = 420000
    const warmupPerEpoch = 1.25
    let blocksUntilWarmup = 0
    let epochsUntilWarmup = 0

    if (activatedStake >= totalDelegated) {
      res.epochsUntilWarmup = epochsUntilWarmup
      res.msUntilWarmup = blocksUntilWarmup*blockTime*1000
      return res
    }

    while (true) {
      activatedStake = activatedStake*warmupPerEpoch
      if (activatedStake >= totalDelegated) {break}
      blocksUntilWarmup+=epochBlocks
      epochsUntilWarmup+=1
    }

    res.epochsUntilWarmup = epochsUntilWarmup
    res.msUntilWarmup = blocksUntilWarmup*blockTime*1000+msUntilNextEpoch

    return res
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


    return (
        <Fragment>

          {/*<Tooltips/>*/}

          {/* STAKE WARMUP */}
          <div className="container py-4 mt-4">
            <Card className="grey-card pl-2 bg-beach-colors">
              <Row>
                {/* EXPLAINER */}
                <Col md="6" lg="4">
                  <Card className="card-box border-0 text-light">
                    <CardBody>
                      <div className="align-box-row align-items-start text-black">
                    <span className="text-black d-block mb-1 text-uppercase font-weight-600">
                      Road to staking rewards
                    </span>
                      </div>
                      <div className="align-box-row align-items-start text-black">
                        <span className="text-black font-size-sm">Active stake is limited to a 25% increase per epoch. As it will take time for your stake to warm up, now is the time to delegate. Once all stake is warmed up, staking rewards can be enabled! </span>
                      </div>
                    </CardBody>
                  </Card>
                </Col>
                {/* HELP SECTION */}
                <Col md="6" lg="4">
                  <Card className="card-box border-0 text-light">
                    <CardBody>
                      <div className="align-box-row align-items-start text-black">
                    <span className="text-black d-block mb-1 text-uppercase font-weight-600">
                      Resources & Help
                    </span>
                      </div>
                      <div className="text-black">
                        <div className="text-black font-size-sm my-1">
                          <a className="resource-link" href="https://docs.solana.com/staking"  target="_blank">
                            <FontAwesomeIcon icon={['fas', 'book']} className="text-black mr-2"/>Official Staking Docs
                          </a>
                        </div>
                        <div className="text-black font-size-sm my-1">
                          <a className="resource-link" href="https://forums.solana.com/t/validator-information-thread"  target="_blank">
                            <FontAwesomeIcon icon={['fa', 'info-circle']} className="text-black mr-2"/>Official Validator Info Thread
                          </a>
                        </div>
                        <div className="text-black font-size-sm my-1">
                          <a className="resource-link" href="https://discordapp.com/invite/pquxPsq"  target="_blank">
                            <FontAwesomeIcon icon={['fab', 'discord']} className="text-black mr-2"/>Official Discord
                          </a>
                        </div>
                        <div className="text-black font-size-sm my-1">
                          <a className="resource-link" href="https://t.me/stakingfacilities"  target="_blank">
                            <FontAwesomeIcon icon={['fab', 'telegram']} className="text-black mr-2"/>Staking Facilities Telegram
                          </a>
                        </div>
                      </div>
                    </CardBody>
                  </Card>
                </Col>

                {/* WARMUP PROGRESS */}
                <Col md="12" lg="4">
                  <Card className="card-box border-0 text-white">
                    <CardBody>
                      <div className="align-box-row align-items-start">
                        <div className="font-weight-bold w-100">
                          <small className="text-black d-flex justify-content-between mb-3 text-uppercase font-weight-500">
                            <span>Delegated: <span className="text font-size-sm font-weight-bold palegreen">{this.nFormatter(this.state.RPCdata.totalDelegatedStake,1)}<span>&nbsp;SOL</span></span></span>
                            <span>Warmed Up: <span className="text font-size-sm font-weight-bold palegreen"><NumberFormat className="" value={this.state.RPCdata.stakeWarmup.warmedUpStakePercentage} displayType={'text'} thousandSeparator={true} decimalScale={1} suffix={'%'}/></span></span>
                          </small>
                          <Progress animated className="border-grey-bg grey-900 bg-coralred" value={this.state.RPCdata.stakeWarmup.warmedUpStakePercentage} />
                        </div>
                      </div>
                      <div className="mt-4">
                      <span className="palegreen font-size-sm font-weight-bold px-1">
                        {moment.duration(this.state.RPCdata.stakeWarmup.msUntilWarmup, "milliseconds").format("d[d]  h[h]  m[m]")}
                      </span>
                        <span className="text-black font-size-sm font-weight-500"> until full warm up</span>
                      </div>
                    </CardBody>
                  </Card>
                </Col>
              </Row>
            </Card>
          </div>

          <div className="container pb-4">
            <Card className="grey-card pl-2">
              <div className="card-indicator bg-palegreen"></div>
              <Row>

                {/* Isolate Block Height since it updates multiple times per second */}
                <BlockHeight socket={this.props.socket} event="rootNotification"/>

                {/* BLOCK TIMES */}
                <Col md="6" lg="4">
                  <Card className="card-box border-0 text-light">
                    <CardBody>
                      <div className="align-box-row align-items-start">
                        <div className="font-weight-bold">
                        <span className="d-block">
                        <small className="ghostwhite mb-1 text-uppercase">
                          Block time
                        </small>

                          {/*<span id="tooltipContent1" className="ml-2">
                            <FontAwesomeIcon icon={['fa', 'question-circle']} className="palegreen"/>
                        </span>*/}
                          </span>
                          <span className="font-size-xxl mt-1 palegreen"><NumberFormat className="" value={this.state.RPCdata.avgBlockTime_1min} displayType={'text'} thousandSeparator={true} decimalScale={2} suffix={'s'}/></span>
                        </div>
                        <div className="ml-auto">
                          <div className="border-grey-bg text-center text-primary font-size-xl d-50 rounded-circle">
                            <FontAwesomeIcon icon={['fas', 'hourglass-half']} className="palegreen"/>
                          </div>
                        </div>
                      </div>
                      <div className="mt-3">
                        <span className="ghostwhite font-size-sm">1 hour average is </span>
                        <span className="palegreen font-size-sm font-weight-bold px-1"><NumberFormat className="" value={this.state.RPCdata.avgBlockTime_1h} displayType={'text'} thousandSeparator={true} decimalScale={2} suffix={'s'}/></span>
                      </div>
                    </CardBody>
                  </Card>
                </Col>

                {/* EPOCH PROGRESS */}
                <Col md="12" lg="4">
                  <Card className="card-box border-0 text-white">
                    <CardBody>
                      <div className="align-box-row align-items-start">
                        <div className="font-weight-bold w-100">
                          <small className="ghostwhite d-flex justify-content-between mb-3 text-uppercase">
                            <span>Epoch: <span className="text font-size-sm font-weight-bold palegreen">{this.state.RPCdata.epoch}</span></span>
                            <span>Progress: <span className="text font-size-sm font-weight-bold palegreen"><NumberFormat className="" value={this.state.RPCdata.epochPercent} displayType={'text'} thousandSeparator={true} decimalScale={0} suffix={'%'}/></span></span>
                          </small>
                          <Progress animated className="border-grey-bg grey-900 bg-coralred" value={this.state.RPCdata.epochPercent} />
                        </div>
                      </div>
                      <div className="mt-4">
                      <span className="palegreen font-size-sm font-weight-bold px-1">
                        {moment.duration(this.state.RPCdata.msUntilNextEpoch, "milliseconds").format("d[d]  h[h]  m[m]")}
                      </span>
                        <span className="ghostwhite font-size-sm"> until new epoch</span>
                      </div>
                    </CardBody>
                  </Card>
                </Col>
              </Row>

              {/* Isolate LeaderWorldMap since it updates multiple times per second */}
              <LeaderWorldMap socket={this.props.socket}/>

            </Card>
          </div>

          <div className="container pb-4">
            <Card className="grey-card pl-2">
              <div className="card-indicator primary-blue-bg"></div>
              <Row>

                {/* Circulating Supply Rate */}
                <Col md="6" lg="4">
                  <Card className="card-box border-0 text-light">
                    <CardBody>
                      <div className="align-box-row align-items-start">
                        <div className="font-weight-bold">
                          <small className="ghostwhite d-block mb-1 text-uppercase">
                            Circulating Supply
                          </small>
                          <span className="font-size-xxl mt-1 primary-blue">{this.nFormatter(this.state.RPCdata.circulatingSupply,1)}</span>
                          <span className="font-size-md lightgrey"> /{this.nFormatter(this.state.RPCdata.totalSupply,1)}</span>
                        </div>
                        <div className="ml-auto">
                          <div className="border-grey-bg text-center text-primary font-size-xl d-50 rounded-circle">
                            <FontAwesomeIcon icon={['fas', 'sync']} className="primary-blue"/>
                          </div>
                        </div>
                      </div>
                      <div className="mt-3">
                        {/* TODO: implement effective staking return*/}
                        <span className="primary-blue font-size-sm font-weight-bold px-1">
                        <NumberFormat className="" value={(this.state.RPCdata.circulatingSupply/this.state.RPCdata.totalSupply)*100} displayType={'text'} thousandSeparator={true} decimalScale={1} suffix={'%'}/>
                      </span>
                        <span className="ghostwhite font-size-sm">is circulating</span>
                      </div>
                    </CardBody>
                  </Card>
                </Col>

                {/* Staked SOL */}
                <Col md="6" lg="4">
                  <Card className="card-box border-0 text-light">
                    <CardBody>
                      <div className="align-box-row align-items-start justify-content-between">
                        <div className="font-weight-bold">
                          <small className="ghostwhite d-block mb-1 text-uppercase">
                            <span>Active Stake</span>
                          </small>
                          <span className="font-size-xxl mt-1 primary-blue">
                          <span>{this.nFormatter(this.state.RPCdata.activatedStake,1)}</span>
                          <span className="font-size-md lightgrey"> /{this.nFormatter(this.state.RPCdata.totalSupply,1)}</span>
                        </span>
                        </div>
                        <div className="border-grey-bg text-center text-success font-size-xl d-50 rounded-circle">
                          <FontAwesomeIcon icon={['fas', 'coins']} className="primary-blue"/>
                        </div>
                      </div>
                      <div className="mt-3">
                      <span className="ghostwhite font-size-sm">
                      <span>Delinquent stake: <span className={((this.state.RPCdata.delinquentStake/this.state.RPCdata.activatedStake)*100) >= 5 ? "text font-size-sm font-weight-bold coralred" : "text font-size-sm font-weight-bold lightgrey"}><NumberFormat className="" value={(this.state.RPCdata.delinquentStake/this.state.RPCdata.activatedStake)*100} displayType={'text'} thousandSeparator={true} decimalScale={1} suffix={'%'}/></span>
                      </span>
                      </span>
                      </div>
                    </CardBody>
                  </Card>
                </Col>

                {/* Price SOL */}
                <Col md="6" lg="4">
                  <Card className="card-box border-0 text-light">
                    <CardBody>
                      <div className="align-box-row align-items-start">
                        <div className="font-weight-bold">
                          <small className="ghostwhite d-block mb-1 text-uppercase">
                            Price
                          </small>

                          <span>
                          <span className="font-size-xxl mt-1 primary-blue"><NumberFormat className="" value={this.state.RPCdata.tokenPrice} displayType={'text'} thousandSeparator={true} decimalScale={2} prefix={'$'}/></span>
                        </span>

                          {this.state.RPCdata.dailyPriceChange >= 0 ?
                              <span className="pl-3">
                            <FontAwesomeIcon icon={['fas', 'arrow-up']} className="text-success"/>
                            <span className="text-success px-1 font-weight-600">
                              <NumberFormat className="" value={this.state.RPCdata.dailyPriceChange} displayType={'text'} thousandSeparator={true} decimalScale={1} suffix={'%'}/>
                            </span>
                          </span>
                              :
                              <span className="pl-3">
                            <FontAwesomeIcon icon={['fas', 'arrow-down']} className="text-danger"/>
                            <span className="text-danger px-1 font-weight-600">
                              <NumberFormat className="" value={Math.abs(this.state.RPCdata.dailyPriceChange)} displayType={'text'} thousandSeparator={true} decimalScale={1} suffix={'%'}/>
                            </span>
                          </span>
                          }
                        </div>
                        <div className="ml-auto">
                          <div className="border-grey-bg text-center text-primary font-size-xl d-50 rounded-circle">
                            <FontAwesomeIcon icon={['fas', 'dollar-sign']} className="primary-blue"/>
                          </div>
                        </div>
                      </div>

                      <div className="mt-3 font-size-sm ghostwhite">
                        <span className="mr-3">24h Vol: <span className="primary-blue font-size-sm font-weight-bold px-1">${this.nFormatter(this.state.RPCdata.dailyVolume,1)}</span></span>
                        <span>MCap: <span className="primary-blue font-size-sm font-weight-bold px-1">${this.nFormatter(this.state.RPCdata.fullyDilutedMcap)}</span></span>
                      </div>

                    </CardBody>
                  </Card>
                </Col>

              </Row>
            </Card>
          </div>

        </Fragment>

    );
  }
}
