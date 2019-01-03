import PubNub from 'pubnub';
import readline from 'readline';

var rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  terminal: false
});

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

rl.on('line', function(line){
  if (line.indexOf('{"ledger":[') == 0) {
    return;
  }
  var message = JSON.parse(line);
  message.ts = new Date().toISOString();
  message.nodeId = nodeId;

  pubnub.publish({
    channel: 'testbridge',
    message: message
  }, (status, response) => {
    if (status.error) {
      console.log("ERR:\t" + JSON.stringify(status) + "\t" + line);
    } else {
      console.log("OK\t" + JSON.stringify(message));
    }
  });
});

