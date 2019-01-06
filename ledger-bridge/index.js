import PubNub from 'pubnub';
import readline from 'readline';

var pubKey = process.env.PN_PUB_KEY;
var subKey = process.env.PN_SUB_KEY;
var nodeInfo = JSON.parse(process.env.NODE_INFO);
var nodeId = 'node@' + nodeInfo.ip;

console.log("starting: " + nodeId);

var pubnub = new PubNub({
    publishKey: pubKey,
    subscribeKey: subKey,
    uuid: nodeId,
    ssl: true
});

pubnub.subscribe({channels:['testbridge']});
pubnub.setState({channels:['testbridge'],state:nodeInfo});

var dgram = require('dgram');

var HOST = '127.0.0.1';
var PORT = 7654;

var server = dgram.createSocket('udp4');

server.on('listening', function () {
    var address = server.address();
    console.log('UDP Server listening on ' + address.address + ":" + address.port);
});

server.on('message', function (data, remote) {
  var message = JSON.parse(data);
  message.ts = new Date().toISOString();
  message.nodeId = nodeId;

  pubnub.publish({
    channel: 'testbridge',
    message: message
  }, (status, response) => {
    if (status.error) {
      console.log("ERR:\t" + JSON.stringify(status) + "\t" + JSON.stringify(message));
    } else {
      console.log("OK\t" + JSON.stringify(message));
    }
  });
});

server.bind(PORT, HOST);

