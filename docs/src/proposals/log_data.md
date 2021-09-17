# Program log binary data

## Problem

There is no support for logging binary data in Solidity.

### Events in Solidity

In Solidity, events can be reported. These look like structures with zero or
more fields, and can be emitted with specific values. For example:

```
event PaymentReceived {
    address sender;
    uint amount;
}

contract c {
    function pay() public payable {
        emit PaymentReceived(msg.sender, msg.value);
    }
}
```

Events are write-only from a Solidity/VM perspective and are written to
the blocks in the tx records.

Some of these fields can be marked `indexed`, which affects how the data is
encoded. All non-indexed fields are eth abi encoded into a variable length
byte array. All indexed fields go into so-called topics.

Topics are fixed length fields of 32 bytes. There are a maximum of 4 topics;
if a type does not always fit into 32 bytes (e.g. string types), then the topic
is keccak256 hashed.

The first topic is a keccak256 hash of the event signature, in this case
`keccak256('PaymentReceived(address,uint)')`. The four remaining are available
for `indexed` fields. The event may be declared `anonymous`, in which case
the first field is not a hash of the signature, and it is permitted to have
4 indexed fields.

### Listening to events in a client

The reason for the distinction between topics/indexed and regular fields is
that it easier to filter on topics.

```
const Web3 = require('web3');
const url = 'ws://127.0.0.1:8546';
const web3 = new Web3(url);

var options = {
    address: '0xfbBE8f06FAda977Ea1E177da391C370EFbEE3D25',
    topics: [
        '0xdf50c7bb3b25f812aedef81bc334454040e7b27e27de95a79451d663013b7e17',
        //'0x0000000000000000000000000d8a3f5e71560982fb0eb5959ecf84412be6ae3e'
      ]
};

var subscription = web3.eth.subscribe('logs', options, function(error, result){
    if (!error) console.log('got result');
    else console.log(error);
}).on("data", function(log){
    console.log('got data', log);
}).on("changed", function(log){
    console.log('changed');
});
```

In order to decode the non-indexed fields (the data), the abi of the contract
is needed. So, the topic is first used to discover what event was used, and
then the data can be decoded.

### Ethereum Tx in block

The transaction calls event logs. Here is a tx with a single event, with 3
topics and some data.

```
{
  "tx": {
    "nonce": "0x2",
    "gasPrice": "0xf224d4a00",
    "gas": "0xc350",
    "to": "0x6B175474E89094C44Da98b954EedeAC495271d0F",
    "value": "0x0",
    "input": "0xa9059cbb000000000000000000000000a12431d0b9db640034b0cdfceef9cce161e62be40000000000000000000000000000000000000000000000a030dcebbd2f4c0000",
    "hash": "0x98a67f0a35ebc0ac068acf0885d38419c632ffa4354e96641d6d5103a7681910",
    "blockNumber": "0xc96431",
    "from": "0x82f890D638478d211eF2208f3c1466B5Abf83551",
    "transactionIndex": "0xe1"
  },
  "receipt": {
    "gasUsed": "0x74d2",
    "status": "0x1",
    "logs": [
      {
        "address": "0x6B175474E89094C44Da98b954EedeAC495271d0F",
        "topics": [
          "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
          "0x00000000000000000000000082f890d638478d211ef2208f3c1466b5abf83551",
          "0x000000000000000000000000a12431d0b9db640034b0cdfceef9cce161e62be4"
        ],
        "data": "0x0000000000000000000000000000000000000000000000a030dcebbd2f4c0000"
      }
    ]
  }
}
```

### Further considerations

In Ethereum, events are stored in blocks. Events mark certain state changes in
smart contracts. This serves two purposes:

 - Listen to events (i.e. state changes) as they happen by reading new blocks
   as they are published
 - Re-read historical events by reading old blocks

So for example, smart contracts may emit changes as they happen but never the
complete state, so the only way to recover the entire state of the contract
is by re-reading all events from the chain. So an application will read events
from block 1 or whatever block the application was deployed at and then use
that state for local processing. This is a local cache and may re-populated
from the chain at any point.

## Proposed Solution

Binary logging should be added to the program log. The program log should include the base64 encoded data (zero or more one permitted).

So if we encoding the topics first, followed by the data then the event in the
tx above would look like:
```
program data: 3fJSrRviyJtpwrBo/DeNqpUrpFjxKEWKPVaTfUjs8AAAAAAAAAAAAAAACC+JDWOEeNIR7yII88FGa1q/g1UQAAAAAAAAAAAAAAAKEkMdC522QANLDN/O75zOFh5ivk AAAAAAAAAAAAAAAAAAAAAAAAAAAAAACgMNzrvS9MAAA=
```

This requires a new system call:

```
void sol_log_data(SolBytes *fields, uint64_t length);
```

### Considerations

- Should there be text field in the program log so we can have a little bit of
  metadata on the binary data, to make it more human readable
