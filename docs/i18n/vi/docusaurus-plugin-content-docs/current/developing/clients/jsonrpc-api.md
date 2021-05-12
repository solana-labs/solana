---
title: JSON RPC API
---

Các node Solana chấp nhận các yêu cầu HTTP bằng cách sử dụng [JSON-RPC 2.0](https://www.jsonrpc.org/specification).

Để tương tác với node Solana bên trong ứng dụng JavaScript, hãy sử dụng thư viện [solana-web3.js](https://github.com/solana-labs/solana-web3.js), thư viện này cung cấp giao diện thuận tiện cho các phương thức RPC.

## Điểm cuối HTTP RPC

**Cổng mặc định:** 8899 eg. [http://localhost:8899](http://localhost:8899), [http://192.168.1.88:8899](http://192.168.1.88:8899)

## Điểm cuối RPC PubSub WebSocket

**Cổng mặc định:** 8900 eg. ws://localhost:8900, [http://192.168.1.88:8900](http://192.168.1.88:8900)

## Phương pháp

- [getAccountInfo](jsonrpc-api.md#getaccountinfo)
- [getBalance](jsonrpc-api.md#getbalance)
- [getBlock](jsonrpc-api.md#getblock)
- [getBlockProduction](jsonrpc-api.md#getblockproduction)
- [getBlockCommitment](jsonrpc-api.md#getblockcommitment)
- [getBlocks](jsonrpc-api.md#getblocks)
- [getBlocksWithLimit](jsonrpc-api.md#getblockswithlimit)
- [getBlockTime](jsonrpc-api.md#getblocktime)
- [getClusterNodes](jsonrpc-api.md#getclusternodes)
- [getEpochInfo](jsonrpc-api.md#getepochinfo)
- [getEpochSchedule](jsonrpc-api.md#getepochschedule)
- [getFeeCalculatorForBlockhash](jsonrpc-api.md#getfeecalculatorforblockhash)
- [getFeeRateGovernor](jsonrpc-api.md#getfeerategovernor)
- [getFees](jsonrpc-api.md#getfees)
- [getFirstAvailableBlock](jsonrpc-api.md#getfirstavailableblock)
- [getGenesisHash](jsonrpc-api.md#getgenesishash)
- [getHealth](jsonrpc-api.md#gethealth)
- [getIdentity](jsonrpc-api.md#getidentity)
- [getInflationGovernor](jsonrpc-api.md#getinflationgovernor)
- [getInflationRate](jsonrpc-api.md#getinflationrate)
- [getInflationReward](jsonrpc-api.md#getinflationreward)
- [getLargestAccounts](jsonrpc-api.md#getlargestaccounts)
- [getLeaderSchedule](jsonrpc-api.md#getleaderschedule)
- [getMaxRetransmitSlot](jsonrpc-api.md#getmaxretransmitslot)
- [getMaxShredInsertSlot](jsonrpc-api.md#getmaxshredinsertslot)
- [getMinimumBalanceForRentExemption](jsonrpc-api.md#getminimumbalanceforrentexemption)
- [getMultipleAccounts](jsonrpc-api.md#getmultipleaccounts)
- [getProgramAccounts](jsonrpc-api.md#getprogramaccounts)
- [getRecentBlockhash](jsonrpc-api.md#getrecentblockhash)
- [getRecentPerformanceSamples](jsonrpc-api.md#getrecentperformancesamples)
- [getSignaturesForAddress](jsonrpc-api.md#getsignaturesforaddress)
- [getSignatureStatuses](jsonrpc-api.md#getsignaturestatuses)
- [getSlot](jsonrpc-api.md#getslot)
- [getSlotLeader](jsonrpc-api.md#getslotleader)
- [getSlotLeaders](jsonrpc-api.md#getslotleaders)
- [getStakeActivation](jsonrpc-api.md#getstakeactivation)
- [getSupply](jsonrpc-api.md#getsupply)
- [getTokenAccountBalance](jsonrpc-api.md#gettokenaccountbalance)
- [getTokenAccountsByDelegate](jsonrpc-api.md#gettokenaccountsbydelegate)
- [getTokenAccountsByOwner](jsonrpc-api.md#gettokenaccountsbyowner)
- [getTokenLargestAccounts](jsonrpc-api.md#gettokenlargestaccounts)
- [getTokenSupply](jsonrpc-api.md#gettokensupply)
- [getTransaction](jsonrpc-api.md#gettransaction)
- [getTransactionCount](jsonrpc-api.md#gettransactioncount)
- [getVersion](jsonrpc-api.md#getversion)
- [getVoteAccounts](jsonrpc-api.md#getvoteaccounts)
- [minimumLedgerSlot](jsonrpc-api.md#minimumledgerslot)
- [requestAirdrop](jsonrpc-api.md#requestairdrop)
- [sendTransaction](jsonrpc-api.md#sendtransaction)
- [simulateTransaction](jsonrpc-api.md#simulatetransaction)
- [Đăng ký Websocket](jsonrpc-api.md#subscription-websocket)
  - [accountSubscribe](jsonrpc-api.md#accountsubscribe)
  - [accountUnsubscribe](jsonrpc-api.md#accountunsubscribe)
  - [logsSubscribe](jsonrpc-api.md#logssubscribe)
  - [logsUnsubscribe](jsonrpc-api.md#logsunsubscribe)
  - [programSubscribe](jsonrpc-api.md#programsubscribe)
  - [programUnsubscribe](jsonrpc-api.md#programunsubscribe)
  - [signatureSubscribe](jsonrpc-api.md#signaturesubscribe)
  - [signatureUnsubscribe](jsonrpc-api.md#signatureunsubscribe)
  - [slotSubscribe](jsonrpc-api.md#slotsubscribe)
  - [slotUnsubscribe](jsonrpc-api.md#slotunsubscribe)

### Deprecated Methods

- [getConfirmedBlock](jsonrpc-api.md#getconfirmedblock)
- [getConfirmedBlocks](jsonrpc-api.md#getconfirmedblocks)
- [getConfirmedBlocksWithLimit](jsonrpc-api.md#getconfirmedblockswithlimit)
- [getConfirmedSignaturesForAddress2](jsonrpc-api.md#getconfirmedsignaturesforaddress2)
- [getConfirmedTransaction](jsonrpc-api.md#getconfirmedtransaction)

## Yêu cầu Định dạng

Để thực hiện một yêu cầu JSON-RPC, hãy gửi một yêu cầu HTTP POST với một `Content-Type: application/json`. Dữ liệu yêu cầu JSON phải chứa 4 trường sau:

- `jsonrpc: <string>`, đặt thành `"2.0"`
- `id: <number>`, một số nguyên nhận dạng duy nhất do khách hàng tạo
- `method: <string>`, một chuỗi chứa phương thức được gọi
- `params: <array>`, một mảng JSON gồm các giá trị thông số có thứ tự

Ví dụ sử dụng curl:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"
    ]
  }
'
```

Đầu ra phản hồi sẽ là một đối tượng JSON với các trường sau:

- `jsonrpc: <string>`, khớp với yêu cầu mô tả
- `id: <number>`, khớp với yêu cầu số nhận dạng
- `result: <array|number|object|string>`, dữ liệu được yêu cầu hoặc xác nhận thành công

Yêu cầu có thể được gửi theo lô bằng cách gửi một mảng các đối tượng yêu cầu JSON-RPC làm dữ liệu cho một POST.

## Định nghĩa

- Hash: Hàm băm SHA-256 của một đoạn dữ liệu.
- Pubkey: Khóa public key của key-pair Ed25519.
- Transaction: Danh sách các hướng dẫn của Solana được ký bởi keypair khách hàng để cho phép các hành động đó.
- Signature: Chữ ký Ed25519 của dữ liệu trọng tải của giao dịch bao gồm các hướng dẫn. Điều này có thể được sử dụng để xác định các giao dịch.

## Định cấu hình Cam kết của Nhà nước

Để kiểm tra trước khi đi và xử lý giao dịch, các node Solana chọn trạng thái ngân hàng để truy vấn dựa trên yêu cầu cam kết do khách hàng đặt ra. Cam kết mô tả cách hoàn thành một khối tại thời điểm đó. Khi truy vấn trạng thái sổ cái, bạn nên sử dụng mức độ cam kết thấp hơn để báo cáo tiến độ và mức độ cao hơn để đảm bảo trạng thái sẽ không bị lùi lại.

Theo thứ tự cam kết giảm dần (được hoàn thiện nhiều nhất đến ít được hoàn thiện nhất), khách hàng có thể chỉ định:

- `"finalized"` - the node will query the most recent block confirmed by supermajority of the cluster as having reached maximum lockout, meaning the cluster has recognized this block as finalized
- `"confirmed"` - the node will query the most recent block that has been voted on by supermajority of the cluster.
  - Nó kết hợp các phiếu bầu từ gossip và phát lại.
  - Nó không tính phiếu bầu cho con cháu của một khối, chỉ những phiếu bầu trực tiếp trên khối đó.
  - Mức xác nhận này cũng duy trì các đảm bảo "xác nhận lạc quan" trong phiên bản 1.3 trở đi.
- `"processed"` - the node will query its most recent block. Lưu ý rằng khối có thể không hoàn chỉnh.

For processing many dependent transactions in series, it's recommended to use `"confirmed"` commitment, which balances speed with rollback safety. For total safety, it's recommended to use`"finalized"` commitment.

#### Ví dụ

Thông số cam kết phải được bao gồm dưới dạng phần tử cuối cùng trong mảng `params`:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri",
      {
        "commitment": "finalized"
      }
    ]
  }
'
```

#### Mặc định:

If commitment configuration is not provided, the node will default to `"finalized"` commitment

Chỉ các phương thức mà trạng thái ngân hàng truy vấn chấp nhận thông số cam kết. Chúng được chỉ định trong Tham chiếu API bên dưới.

#### Cấu trúc RpcResponse

Nhiều phương thức nhận thông số cam kết trả về đối tượng RpcResponse JSON bao gồm hai phần:

- `context` : Một cấu trúc RpcResponseContext JSON bao gồm một `slot` mà tại đó các hoạt động được đánh giá.
- `value` : Giá trị do chính hoạt động trả về.

## Kiểm tra Sức khỏe

Mặc dù không phải là API JSON RPC, `GET /health` tại Điểm cuối HTTP RPC cung cấp cơ chế kiểm tra tình trạng để các bộ cân bằng tải hoặc cơ sở hạ tầng mạng khác sử dụng. This request will always return a HTTP 200 OK response with a body of "ok", "behind" or "unknown" based on the following conditions:

1. If one or more `--trusted-validator` arguments are provided to `solana-validator`, "ok" is returned when the node has within `HEALTH_CHECK_SLOT_DISTANCE` slots of the highest trusted validator, otherwise "behind". "unknown" is returned when no slot information from trusted validators is not yet available.
2. "ok" luôn được trả về nếu không có các validator đáng tin cậy nào được cung cấp.

## Tham chiếu API JSON RPC

### getAccountInfo

Trả về tất cả thông tin được liên kết với tài khoản của Pubkey đã cung cấp

#### Thông số:

- `<string>` - Pubkey của tài khoản để truy vấn, dưới dạng chuỗi được mã hóa base-58
- `<object>` - (tùy chọn) Đối tượng cấu hình chứa các trường tùy chọn sau:
  - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - mã hóa cho dữ liệu Tài khoản, "base58" (_chậm_), "base64", "base64+zstd", hoặc "jsonParsed". "base58" được giới hạn đối với dữ liệu Tài khoản dưới 129 byte. "base64" sẽ trả về dữ liệu được mã hóa base64 cho dữ liệu Tài khoản ở bất kỳ kích thước nào. "base64 + zstd" nén dữ liệu Tài khoản bằng cách sử dụng [Zstandard](https://facebook.github.io/zstd/) và base64 mã hóa kết quả. Mã hóa "jsonParsed" cố gắng sử dụng trình phân tích cú pháp trạng thái của chương trình cụ thể để trả về dữ liệu trạng thái tài khoản rõ ràng và dễ đọc hơn. Nếu "jsonParsed" được yêu cầu nhưng không tìm thấy trình phân tích cú pháp, trường sẽ trở lại mã hóa "base64", có thể phát hiện được khi `data` trường được nhập `<string>`.
  - (tùy chọn) `dataSlice: <object>` - giới hạn dữ liệu tài khoản trả về bằng cách sử dụng các trường `offset: <usize>` và `length: <usize>`; chỉ khả dụng cho các mã hóa "base58", "base64" hoặc "base64 + zstd".

#### Kết quả:

Kết quả sẽ là một đối tượng JSON RpcResponse có giá trị `value` bằng:

- `<null>` - nếu tài khoản được yêu cầu không tồn tại
- `<object>` - nếu không, một đối tượng JSON chứa:
  - `lamports: <u64>`, số lượng lamport được chỉ định cho tài khoản này, dưới dạng u64
  - `owner: <string>`, Pubkey được mã hóa base-58 của chương trình mà tài khoản này đã được gán cho
  - `data: <[string, encoding]|object>`, dữ liệu được liên kết với tài khoản, dưới dạng dữ liệu nhị phân được mã hóa hoặc định dạng JSON `{<program>: <state>}`, tùy thuộc vào thông số mã hóa
  - `executable: <bool>`, boolean cho biết tài khoản có chứa chương trình hay không \(và ở chế độ chỉ đọc\)
  - `rentEpoch: <u64>`, kỷ nguyên mà tài khoản này sẽ nợ tiền thuê tiếp theo, là u64

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getAccountInfo",
    "params": [
      "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
      {
        "encoding": "base58"
      }
    ]
  }
'
```

Phản ứng:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "data": [
        "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHRTPuR3oZ1EioKtYGiYxpxMG5vpbZLsbcBYBEmZZcMKaSoGx9JZeAuWf",
        "base58"
      ],
      "executable": false,
      "lamports": 1000000000,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getAccountInfo",
    "params": [
      "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA",
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

Phản ứng:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "data": {
        "nonce": {
          "initialized": {
            "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
            "blockhash": "3xLP3jK6dVJwpeGeTDYTwdDK3TKchUf1gYYGHa4sF3XJ",
            "feeCalculator": {
              "lamportsPerSignature": 5000
            }
          }
        }
      },
      "executable": false,
      "lamports": 1000000000,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

### getBalance

Trả về số dư của tài khoản của Pubkey đã cung cấp

#### Thông số:

- `<string>` - Pubkey của tài khoản để truy vấn, dưới dạng chuỗi được mã hóa base-58
- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

- Đối tượng RpcResponse JSON với `value` được đặt thành số dư

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": { "context": { "slot": 1 }, "value": 0 },
  "id": 1
}
```

### getBlock

Trả về thông tin nhận dạng và giao dịch về một khối được xác nhận trong sổ cái

#### Thông số:

- `<u64>` - slot, dưới dạng số nguyên u64
- `<object>` - (tùy chọn) Đối tượng cấu hình chứa các trường tùy chọn sau:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". Nếu tham số không được cung cấp, mã hóa mặc định là "json". Mã hóa "jsonParsed" cố gắng sử dụng trình phân tích cú pháp hướng dẫn của chương trình cụ thể để trả về dữ liệu rõ ràng và dễ đọc hơn trong danh sách `transaction.message.instructions`. Nếu "jsonParsed" được yêu cầu nhưng một phân tích cú pháp không thể được tìm thấy, các hướng dẫn rơi trở lại để mã hóa JSON thường xuyên (`accounts`, `data`, và `programIdIndex`).
  - (optional) `transactionDetails: <string>` - level of transaction detail to return, either "full", "signatures", or "none". If parameter not provided, the default detail level is "full".
  - (optional) `rewards: bool` - whether to populate the `rewards` array. If parameter not provided, the default includes rewards.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Kết quả:

Trường kết quả sẽ là một đối tượng với các trường sau:

- `<null>` - nếu khối được chỉ định không được xác nhận
- `<object>` - nếu khối được xác nhận, một đối tượng có các trường sau:
  - `blockhash: <string>` - blockhash của khối này, dưới dạng chuỗi được mã hóa base-58
  - `previousBlockhash: <string>` - blockhash của khối cha mẹ này, dưới dạng chuỗi được mã hóa base-58; nếu khối mẹ không khả dụng do dọn dẹp sổ cái, trường này sẽ trả về "11111111111111111111111111111111"
  - `parentSlot: <u64>` - chỉ số slot của khối cha mẹ này
  - `transactions: <array>` - present if "full" transaction details are requested; an array of JSON objects containing:
    - `transaction: <object|[string,encoding]>` - [Đối tượng giao dịch](#transaction-structure), ở định dạng JSON hoặc dữ liệu nhị phân được mã hóa, tùy thuộc vào thông số mã hóa
    - `meta: <object>` - đối tượng siêu dữ liệu trạng thái giao dịch, chứa `null` hoặc:
      - `err: <object | null>` - Error nếu giao dịch không thành công, null nếu giao dịch thành công. [Các định nghĩa của TransactionError](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - `fee: <u64>` - phí giao dịch này đã được tính, dưới dạng số nguyên u64
      - `preBalances: <array>` - mảng số dư tài khoản u64 từ trước khi giao dịch được xử lý
      - `postBalances: <array>` - mảng số dư tài khoản u64 sau khi giao dịch được xử lý
      - `innerInstructions: <array|undefined>` - Danh sách [các hướng dẫn bên trong](#inner-instructions-structure) hoặc bị bỏ qua nếu ghi hướng dẫn bên trong chưa được bật trong giao dịch này
      - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `logMessages: <array>` - mảng thông báo nhật ký chuỗi hoặc bị bỏ qua nếu tính năng ghi thông báo nhật ký chưa được bật trong giao dịch này
      - DEPRECATED: `status: <object>` - Trạng thái giao dịch
        - `"Ok": <null>` - Giao dịch thành công
        - `"Err": <ERR>` - Giao dịch không thành công với TransactionError
  - `signatures: <array>` - present if "signatures" are requested for transaction details; an array of signatures strings, corresponding to the transaction order in the block
  - `rewards: <array>` - present if rewards are requested; an array of JSON objects containing:
    - `pubkey: <string>` - Public key, dưới dạng chuỗi được mã hóa base-58, của tài khoản nhận được phần thưởng
    - `lamports: <i64>` - số lượng lamport thưởng được ghi có hoặc ghi nợ bởi tài khoản, dưới dạng i64
    - `postBalance: <u64>` - số dư tài khoản trong các lamport sau khi áp dụng phần thưởng
    - `rewardType: <string|undefined>` - loại phần thưởng: "phí", "thuê", "bỏ phiếu", "staking"
  - `blockTime: <i64 | null>` - thời gian sản xuất ước tính, dưới dạng dấu thời gian Unix (giây kể từ kỷ nguyên Unix). null nếu không có sẵn

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlock","params":[430, {"encoding": "json","transactionDetails":"full","rewards":false}]}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
          "status": {
            "Ok": null
          }
        },
        "transaction": {
          "message": {
            "accountKeys": [
              "3UVYmECPPMZSCqWKfENfuoTv51fTDTWicX9xmBD2euKe",
              "AjozzgE83A3x1sHNUR64hfH7zaEBWeMaFuAN9kQgujrc",
              "SysvarS1otHashes111111111111111111111111111",
              "SysvarC1ock11111111111111111111111111111111",
              "Vote111111111111111111111111111111111111111"
            ],
            "header": {
              "numReadonlySignedAccounts": 0,
              "numReadonlyUnsignedAccounts": 3,
              "numRequiredSignatures": 1
            },
            "instructions": [
              {
                "accounts": [1, 2, 3, 0],
                "data": "37u9WtQpcm6ULa3WRQHmj49EPs4if7o9f1jSRVZpm2dvihR9C8jY4NqEwXUbLwx15HBSNcP1",
                "programIdIndex": 4
              }
            ],
            "recentBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B"
          },
          "signatures": [
            "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv"
          ]
        }
      }
    ]
  },
  "id": 1
}
```

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlock","params":[430, "base64"]}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
          "status": {
            "Ok": null
          }
        },
        "transaction": [
          "AVj7dxHlQ9IrvdYVIjuiRFs1jLaDMHixgrv+qtHBwz51L4/ImLZhszwiyEJDIp7xeBSpm/TX5B7mYzxa+fPOMw0BAAMFJMJVqLw+hJYheizSoYlLm53KzgT82cDVmazarqQKG2GQsLgiqktA+a+FDR4/7xnDX7rsusMwryYVUdixfz1B1Qan1RcZLwqvxvJl4/t3zHragsUp0L47E24tAFUgAAAABqfVFxjHdMkoVmOYaR1etoteuKObS21cc1VbIQAAAAAHYUgdNXR0u3xNdiTr072z2DVec9EQQ/wNo1OAAAAAAAtxOUhPBp2WSjUNJEgfvy70BbxI00fZyEPvFHNfxrtEAQQEAQIDADUCAAAAAQAAAAAAAACtAQAAAAAAAAdUE18R96XTJCe+YfRfUp6WP+YKCy/72ucOL8AoBFSpAA==",
          "base64"
        ]
      }
    ]
  },
  "id": 1
}
```

#### Cơ cấu giao dịch

Các giao dịch khá khác so với các giao dịch trên các blockchain khác. Hãy nhớ xem lại [Phân tích giao dịch](developing/programming-model/transactions.md) để tìm hiểu về giao dịch trên Solana.

Cấu trúc JSON của một giao dịch được định nghĩa như sau:

- `signatures: <array[string]>` - Danh sách các chữ ký được mã hóa base-58 được áp dụng cho giao dịch. Danh sách luôn có độ dài `message.header.numRequiredSignatures` và không được để trống. Chữ ký tại chỉ mục `i` tương ứng với public key tại chỉ mục `i` trong `message.account_keys`. Cái đầu tiên được sử dụng làm [id giao dịch](../../terminology.md#transaction-id).
- `message: <object>` - Xác định nội dung của giao dịch.
  - `accountKeys: <array[string]>` - Danh sách các public key được mã hóa base-58 được sử dụng bởi giao dịch, bao gồm cả hướng dẫn và chữ ký. Các public key `message.header.numRequiredSignatures` đầu tiên phải ký vào giao dịch.
  - `header: <object>` - Chi tiết các loại tài khoản và chữ ký mà giao dịch yêu cầu.
    - `numRequiredSignatures: <number>` - Tổng số chữ ký cần thiết để giao dịch có hiệu lực. Chữ ký phải khớp với chữ ký đầu tiên `numRequiredSignatures` của `message.account_keys`.
    - `numReadonlySignedAccounts: <number>` - Khóa cuối cùng `numReadonlySignedAccounts` của các khóa đã ký là tài khoản chỉ đọc. Các chương trình có thể xử lý nhiều giao dịch tải tài khoản-chỉ đọc trong một mục nhập PoH, nhưng không được phép ghi có hoặc ghi nợ các lamport hoặc sửa đổi dữ liệu tài khoản. Các giao dịch nhắm mục tiêu đến cùng một tài khoản đọc-ghi được đánh giá tuần tự.
    - `numReadonlyUnsignedAccounts: <number>` - Khóa cuối cùng `numReadonlyUnsignedAccounts` của các khóa chưa được đánh dấu là tài khoản-chỉ đọc.
  - `recentBlockhash: <string>` - Một hàm băm được mã hóa base-58 của một khối gần đây trong sổ cái được sử dụng để ngăn chặn sự trùng lặp giao dịch và cung cấp cho thời gian tồn tại của giao dịch.
  - `instructions: <array[object]>` - Danh sách các hướng dẫn chương trình sẽ được thực hiện theo trình tự và được cam kết trong một giao dịch nguyên tử nếu tất cả đều thành công.
    - `programIdIndex: <number>` - Chỉ vào `message.accountKeys` cho biết tài khoản chương trình thực hiện lệnh này.
    - `accounts: <array[number]>` - Danh sách các chỉ số có thứ tự vào `message.accountKeys` cho biết tài khoản nào cần chuyển vào chương trình.
    - `data: <string>` - Dữ liệu đầu vào của chương trình được mã hóa trong chuỗi base-58.

#### Cấu trúc hướng dẫn bên trong

Thời gian chạy Solana ghi lại các hướng dẫn chương trình chéo được gọi trong quá trình xử lý giao dịch và làm cho chúng có sẵn để minh bạch hơn những gì đã được thực hiện trên chuỗi cho mỗi lệnh giao dịch. Các lệnh được gọi được nhóm theo lệnh giao dịch gốc và được liệt kê theo thứ tự xử lý.

Cấu trúc JSON của các lệnh bên trong được định nghĩa là danh sách các đối tượng trong cấu trúc sau:

- `index: number` - Chỉ mục của lệnh giao dịch mà các lệnh bên trong bắt nguồn từ đó
- `instructions: <array[object]>` - Danh sách có thứ tự các lệnh chương trình bên trong đã được gọi trong một lệnh giao dịch duy nhất.
  - `programIdIndex: <number>` - Chỉ vào `message.accountKeys` cho biết tài khoản chương trình thực hiện lệnh này.
  - `accounts: <array[number]>` - Danh sách các chỉ số có thứ tự vào `message.accountKeys` cho biết tài khoản nào cần chuyển vào chương trình.
  - `data: <string>` - Dữ liệu đầu vào của chương trình được mã hóa trong chuỗi base-58.

#### Token Balances Structure

The JSON structure of token balances is defined as a list of objects in the following structure:

- `accountIndex: <number>` - Index of the account in which the token balance is provided for.
- `mint: <string>` - Pubkey of the token's mint.
- `uiTokenAmount: <object>` -
  - `amount: <string>` - Raw amount of tokens as a string, ignoring decimals.
  - `decimals: <number>` - Number of decimals configured for token's mint.
  - `uiAmount: <number | null>` - Token amount as a float, accounting for decimals. **DEPRECATED**
  - `uiAmountString: <string>` - Token amount as a string, accounting for decimals.

### getBlockProduction

Returns recent block production information from the current or previous epoch.

#### Thông số:

- `<object>` - (tùy chọn) Đối tượng cấu hình chứa các trường tùy chọn sau:
  - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `range: <object>` - Slot range to return block production for. Nếu thông số không được cung cấp, mặc định là kỷ nguyên hiện tại.
    - `firstSlot: <u64>` - first slot to return block production information for (inclusive)
    - (optional) `lastSlot: <u64>` - last slot to return block production information for (inclusive). If parameter not provided, defaults to the highest slot
  - (optional) `identity: <string>` - Only return results for this validator identity (base-58 encoded)

#### Kết quả:

Kết quả sẽ là một đối tượng JSON RpcResponse có giá trị `value` bằng:

- `<object>`
  - `byIdentity: <object>` - a dictionary of validator identities, as base-58 encoded strings. Value is a two element array containing the number of leader slots and the number of blocks produced.
  - `range: <object>` - Block production slot range
    - `firstSlot: <u64>` - first slot of the block production information (inclusive)
    - `lastSlot: <u64>` - last slot of block production information (inclusive)

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockProduction"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 9887
    },
    "value": {
      "byIdentity": {
        "85iYT5RuzRTDgjyRa3cP8SYhM2j21fj7NhfJ3peu1DPr": [9888, 9886]
      },
      "range": {
        "firstSlot": 0,
        "lastSlot": 9887
      }
    }
  },
  "id": 1
}
```

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBlockProduction",
    "params": [
      {
        "identity": "85iYT5RuzRTDgjyRa3cP8SYhM2j21fj7NhfJ3peu1DPr",
        "range": {
          "firstSlot": 40,
          "lastSlot": 50
        }
      }
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 10102
    },
    "value": {
      "byIdentity": {
        "85iYT5RuzRTDgjyRa3cP8SYhM2j21fj7NhfJ3peu1DPr": [11, 11]
      },
      "range": {
        "firstSlot": 50,
        "lastSlot": 40
      }
    }
  },
  "id": 1
}
```

### getBlockCommitment

Trả về cam kết cho khối cụ thể

#### Thông số:

- `<u64>` - khối, được xác định bởi Slot

#### Kết quả:

Trường kết quả sẽ là một đối tượng JSON chứa:

- `commitment` - cam kết, bao gồm:
  - `<null>` - Khối không xác định
  - `<array>` - cam kết, mảng số nguyên u64 ghi lại số lượng stake cụm trong các lamport đã bỏ phiếu cho khối ở mỗi độ sâu từ 0 đến `MAX_LOCKOUT_HISTORY` + 1
- `totalStake` - tổng stake hoạt động, trong các lamport, của kỷ nguyên hiện tại

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "commitment": [
      0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      0, 0, 0, 0, 0, 10, 32
    ],
    "totalStake": 42
  },
  "id": 1
}
```

### getBlocks

Trả về danh sách các khối được xác nhận giữa hai slot

#### Thông số:

- `& lt; u64 & gt;` - start_slot, dưới dạng số nguyên u64
- `<u64>` - (tùy chọn) end_slot, dưới dạng số nguyên u64
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Kết quả:

Kết quả sẽ là một mảng các số nguyên u64 liệt kê các khối đã được xác nhận giữa `start_slot` và `end_slot`, nếu được cung cấp, hoặc khối được xác nhận mới nhất, bao gồm. Phạm vi tối đa cho phép là 500,000 slot.

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlocks","params":[5, 10]}
'
```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7, 8, 9, 10], "id": 1 }
```

### getBlocksWithLimit

Trả về danh sách các khối được xác nhận bắt đầu tại slot đã cho

#### Thông số:

- `<u64>` - start_slot, dưới dạng số nguyên u64
- `<u64>` - giới hạn, dưới dạng số nguyên u64
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Kết quả:

Trường kết quả sẽ là một mảng các số nguyên u64 liệt kê các khối đã được xác nhận bắt đầu từ `start_slot` đến tối đa `limit` các khối, bao gồm.

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlocksWithLimit","params":[5, 3]}
'
```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7], "id": 1 }
```

### getBlockTime

Returns the estimated production time of a block.

Mỗi validator báo cáo thời gian UTC của họ vào sổ cái theo một khoảng thời gian đều đặn bằng cách liên tục thêm dấu thời gian vào phiếu bầu cho một khối cụ thể. Thời gian của khối được yêu cầu được tính từ giá trị trung bình có tỷ trọng của các dấu thời gian Bỏ phiếu trong một tập hợp các khối gần đây được ghi lại trên sổ cái.

#### Thông số:

- `<u64>` - khối, được xác định bởi Slot

#### Kết quả:

- `<i64>` - thời gian sản xuất ước tính, dưới dạng dấu thời gian Unix (giây kể từ kỷ nguyên Unix)
- `<null>` - dấu thời gian không có sẵn cho khối này

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockTime","params":[5]}
'
```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": 1574721591, "id": 1 }
```

### getClusterNodes

Trả về thông tin về tất cả các node tham gia vào cụm

#### Thông số:

Không có

#### Kết quả:

Trường kết quả sẽ là một mảng các đối tượng JSON, mỗi đối tượng có các trường con sau:

- `pubkey: <string>` - Node public key, dưới dạng chuỗi được mã hóa base-58
- `gossip: <string>` - Địa chỉ mạng gossip cho node
- `tpu: <string>` - Địa chỉ mạng TPU cho node
- `rpc: <string>|null` - Địa chỉ mạng JSON RPC cho node hoặc `null` nếu dịch vụ JSON RPC không được bật
- `version: <string>|null` - Phiên bản phần mềm của node hoặc `null` nếu thông tin phiên bản không có sẵn

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getClusterNodes"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "gossip": "10.239.6.48:8001",
      "pubkey": "9QzsJf7LPLj8GkXbYT3LFDKqsj2hHG7TA3xinJHu8epQ",
      "rpc": "10.239.6.48:8899",
      "tpu": "10.239.6.48:8856",
      "version": "1.0.0 c375ce1f"
    }
  ],
  "id": 1
}
```

### getEpochInfo

Trả về thông tin về kỷ nguyên hiện tại

#### Thông số:

- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

Trường kết quả sẽ là một đối tượng với các trường sau:

- `absoluteSlot: <u64>`, slot hiện tại
- `blockHeight: <u64>`, chiều cao khối hiện tại
- `epoch: <u64>`, kỷ nguyên hiện tại
- `slotIndex: <u64>`, slot hiện tại so với thời điểm bắt đầu của kỷ nguyên hiện tại
- `slotsInEpoch: <u64>`, số lượng slot trong kỷ nguyên này

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "absoluteSlot": 166598,
    "blockHeight": 166500,
    "epoch": 27,
    "slotIndex": 2790,
    "slotsInEpoch": 8192
  },
  "id": 1
}
```

### getEpochSchedule

Trả về thông tin lịch biểu kỷ nguyên từ cấu hình genesis của cụm này

#### Thông số:

Không có

#### Kết quả:

Trường kết quả sẽ là một đối tượng với các trường sau:

- `slotPerEpoch: & lt; u64 & gt;`, số lượng slot tối đa trong mỗi kỷ nguyên
- `leaderScheduleSlotOffset: <u64>`, số lượng slot trước khi bắt đầu một kỷ nguyên để tính toán lịch trình của leader cho kỷ nguyên đó
- `warmup: <bool>`, liệu các kỷ nguyên bắt đầu ngắn và phát triển
- `firstNormalEpoch: <u64>`, kỷ nguyên có độ dài-bình thường-đầu tiên, log2(slotsPerEpoch) - log2(MINIMUM_SLOTS_PER_EPOCH)
- `firstNormalSlot: <u64>`, MINIMUM_SLOTS_PER_EPOCH \* (2.pow(firstNormalEpoch) - 1)

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochSchedule"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "firstNormalEpoch": 8,
    "firstNormalSlot": 8160,
    "leaderScheduleSlotOffset": 8192,
    "slotsPerEpoch": 8192,
    "warmup": true
  },
  "id": 1
}
```

### getFeeCalculatorForBlockhash

Trả về công cụ tính phí được liên kết với blockhash truy vấn hoặc `null` nếu blockhash đã hết hạn

#### Thông số:

- `<string>` - blockhash truy vấn dưới dạng một chuỗi được mã hóa Base58
- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

Kết quả sẽ là một đối tượng JSON RpcResponse có giá trị `value` bằng:

- `<null>` - nếu blockhash truy vấn đã hết hạn
- `<object>` - nếu không, một đối tượng JSON chứa:
  - `feeCalculator: <object>`, `FeeCalculator` mô tả tỉ lệ phí cụm tại blockhash được truy vấn

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getFeeCalculatorForBlockhash",
    "params": [
      "GJxqhuxcgfn5Tcj6y3f8X4FeCDd2RQ6SnEMo1AAxrPRZ"
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 221
    },
    "value": {
      "feeCalculator": {
        "lamportsPerSignature": 5000
      }
    }
  },
  "id": 1
}
```

### getFeeRateGovernor

Trả về thống đốc thông tin tỷ lệ phí từ ngân hàng gốc

#### Thông số:

Không có

#### Kết quả:

Trường `result` sẽ là một `object` với các trường sau:

- `burnPercent: <u8>`, Phần trăm phí thu được để tiêu hủy
- `maxLamportsPerSignature: <u64>`, Giá trị lớn nhất mà `lamportsPerSignature` có thể đạt được cho slot tiếp theo
- `minLamportsPerSignature: <u64>`, Giá trị nhỏ nhất mà `lamportsPerSignature` có thể đạt được cho slot tiếp theo
- `targetLamportsPerSignature: <u64>`, Tỷ lệ phí mong muốn cho cụm
- `targetSignaturesPerSlot: <u64>`, Tỷ lệ chữ ký mong muốn cho cụm

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFeeRateGovernor"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 54
    },
    "value": {
      "feeRateGovernor": {
        "burnPercent": 50,
        "maxLamportsPerSignature": 100000,
        "minLamportsPerSignature": 5000,
        "targetLamportsPerSignature": 10000,
        "targetSignaturesPerSlot": 20000
      }
    }
  },
  "id": 1
}
```

### getFees

Trả về băm khối gần đây từ sổ cái, một biểu phí có thể được sử dụng để tính toán chi phí gửi giao dịch bằng cách sử dụng nó và slot cuối cùng trong đó blockhash sẽ hợp lệ.

#### Thông số:

- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

Kết quả sẽ là một đối tượng RpcResponse JSON với `value` được đặt thành đối tượng JSON với các trường sau:

- `blockhash: <string>` - một Hash dưới dạng chuỗi được mã hóa base-58
- `feeCalculator: <object>` - FeeCalculator, biểu phí cho hàm băm khối này
- `lastValidSlot: <u64>` - DEPRECATED - this value is inaccurate and should not be relied upon

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "blockhash": "CSymwgTNX1j3E4qhKfJAUE41nBWEwXufoYryPbkde5RR",
      "feeCalculator": {
        "lamportsPerSignature": 5000
      },
      "lastValidSlot": 297
    }
  },
  "id": 1
}
```

### getFirstAvailableBlock

Trả về slot của khối được xác nhận thấp nhất chưa bị xóa khỏi sổ cái

#### Thông số:

Không có

#### Kết quả:

- `<u64>` - Slot

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFirstAvailableBlock"}
'
```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": 250000, "id": 1 }
```

### getGenesisHash

Trả về hàm băm genesis

#### Thông số:

Không có

#### Kết quả:

- `<string>` - một chuỗi mã hóa hàm băm dưới dạng base-58

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getGenesisHash"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": "GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC",
  "id": 1
}
```

### getHealth

Trả về tình trạng hiện tại của node.

Nếu một hoặc nhiều đối số `--trusted-validator` được cung cấp cho `solana-validator`, "ok" được trả về khi node có trong các slot `HEALTH_CHECK_SLOT_DISTANCE` của validator đáng tin cậy cao nhất, nếu không sẽ trả về lỗi. ok" luôn được trả về nếu không có các validator đáng tin cậy nào được cung cấp.

#### Thông số:

Không có

#### Kết quả:

Nếu node healthy: "ok" Nếu node unhealthy, phản hồi lỗi JSON RPC sẽ được trả về. Các chi tiết cụ thể của phản hồi lỗi là **UNSTABLE** và có thể thay đổi trong tương lai

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getHealth"}
'
```

Kết quả Healthy:

```json
{ "jsonrpc": "2.0", "result": "ok", "id": 1 }
```

Kết quả Unhealthy (chung chung):

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32005,
    "message": "Node is unhealthy",
    "data": {}
  },
  "id": 1
}
```

Kết quả Unhealthy (nếu có thêm thông tin)

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32005,
    "message": "Node is behind by 42 slots",
    "data": {
      "numSlotsBehind": 42
    }
  },
  "id": 1
}
```

### getIdentity

Trả về pubkey nhận dạng cho node hiện tại

#### Thông số:

Không có

#### Kết quả:

Trường kết quả sẽ là một đối tượng JSON với các trường sau:

- `identity`, pubkey node nhận dạng của node hiện tại \(dưới dạng chuỗi được mã hóa cơ sở 58\)

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getIdentity"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": { "identity": "2r1F4iWqVcb8M1DbAjQuFpebkQHY9hcVU4WuW2DJBppN" },
  "id": 1
}
```

### getInflationGovernor

Trả về thống đốc lạm phát hiện tại

#### Thông số:

- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

Trường kết quả sẽ là một đối tượng JSON với các trường sau:

- `initial: <f64>`, tỷ lệ phần trăm lạm phát ban đầu từ thời điểm 0
- `terminal: <f64>`, tỷ lệ lạm phát cuối kỳ
- `taper: <f64>`, tỷ lệ lạm phát được hạ thấp mỗi năm. Rate reduction is derived using the target slot time in genesis config
- `foundation: <f64>`, tỷ lệ phần trăm tổng lạm phát được phân bổ cho nền tảng
- `foundationTerm: <f64>`, thời gian lạm phát của nền tảng tính theo năm

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationGovernor"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "foundation": 0.05,
    "foundationTerm": 7,
    "initial": 0.15,
    "taper": 0.15,
    "terminal": 0.015
  },
  "id": 1
}
```

### getInflationRate

Trả về các giá trị lạm phát cụ thể cho kỷ nguyên hiện tại

#### Thông số:

Không có

#### Kết quả:

Trường kết quả sẽ là một đối tượng JSON với các trường sau:

- `total: <f64>`, tổng lạm phát
- `validator: <f64>`, lạm phát được phân bổ cho các validator
- `foundation: <f64>`, lạm phát được phân bổ cho nền tảng
- `epoch: <f64>`, kỷ nguyên mà các giá trị này hợp lệ

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationRate"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "epoch": 100,
    "foundation": 0.001,
    "total": 0.149,
    "validator": 0.148
  },
  "id": 1
}
```

### getInflationReward

Returns the inflation reward for a list of addresses for an epoch

#### Thông số:

- `<array>` - An array of addresses to query, as base-58 encoded strings

* `<object>` - (tùy chọn) Đối tượng cấu hình chứa các trường tùy chọn sau:
  - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `epoch: <u64>` - An epoch for which the reward occurs. If omitted, the previous epoch will be used

#### Kết quả

The result field will be a JSON array with the following fields:

- `epoch: <u64>`, epoch for which reward occured
- `effectiveSlot: <u64>`, the slot in which the rewards are effective
- `amount: <u64>`, reward amount in lamports
- `postBalance: <u64>`, post balance of the account in lamports

#### Ví dụ

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getInflationReward",
    "params": [
       ["6dmNQ5jwLeLk5REvio1JcMshcbvkYMwy26sJ8pbkvStu", "BGsqMegLpV6n6Ve146sSX2dTjUMj3M92HnU8BbNRMhF2"], 2
    ]
  }
'
```

Phản ứng:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "amount": 2500,
      "effectiveSlot": 224,
      "epoch": 2,
      "postBalance": 499999442500
    },
    null
  ],
  "id": 1
}
```

### getLargestAccounts

Returns the 20 largest accounts, by lamport balance (results may be cached up to two hours)

#### Thông số:

- `<object>` - (tùy chọn) Đối tượng cấu hình chứa các trường tùy chọn sau:
  - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)
  - (tùy chọn) `filter: <string>` - lọc kết quả theo loại tài khoản; hiện được hỗ trợ: `circulating|nonCirculating`

#### Kết quả:

Kết quả sẽ là một đối tượng JSON RpcResponse với `value` bằng một mảng:

- `<object>` - nếu không, một đối tượng JSON chứa:
  - `address: <string>`, địa chỉ được mã hóa base-58 của tài khoản
  - `lamports: & lt; u64 & gt;`, số lượng lamport trong tài khoản, dưới dạng u64

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLargestAccounts"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 54
    },
    "value": [
      {
        "lamports": 999974,
        "address": "99P8ZgtJYe1buSK8JXkvpLh8xPsCFuLYhz9hQFNw93WJ"
      },
      {
        "lamports": 42,
        "address": "uPwWLo16MVehpyWqsLkK3Ka8nLowWvAHbBChqv2FZeL"
      },
      {
        "lamports": 42,
        "address": "aYJCgU7REfu3XF8b3QhkqgqQvLizx8zxuLBHA25PzDS"
      },
      {
        "lamports": 42,
        "address": "CTvHVtQ4gd4gUcw3bdVgZJJqApXE9nCbbbP4VTS5wE1D"
      },
      {
        "lamports": 20,
        "address": "4fq3xJ6kfrh9RkJQsmVd5gNMvJbuSHfErywvEjNQDPxu"
      },
      {
        "lamports": 4,
        "address": "AXJADheGVp9cruP8WYu46oNkRbeASngN5fPCMVGQqNHa"
      },
      {
        "lamports": 2,
        "address": "8NT8yS6LiwNprgW4yM1jPPow7CwRUotddBVkrkWgYp24"
      },
      {
        "lamports": 1,
        "address": "SysvarEpochSchedu1e111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "11111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "Stake11111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarC1ock11111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "StakeConfig11111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarRent111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "Config1111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarStakeHistory1111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarRecentB1ockHashes11111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarFees111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "Vote111111111111111111111111111111111111111"
      }
    ]
  },
  "id": 1
}
```

### getLeaderSchedule

Trả về lịch trình của leader cho một epoch

#### Thông số:

- `<u64>` - (tùy chọn) Tìm nạp lịch biểu của leader cho epoch tương ứng với slot được cung cấp. Nếu không được chỉ định, lịch trình của leader cho epoch hiện tại sẽ được tìm nạp
- `<object>` (tùy chọn) Đối tượng cấu hình chứa trường sau:
  - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `identity: <string>` - Only return results for this validator identity (base-58 encoded)

#### Kết quả:

- `<null>` - nếu không tìm thấy epoch được yêu cầu
- `<object>` - otherwise, the result field will be a dictionary of validator identities, as base-58 encoded strings, and their corresponding leader slot indices as values (indices are relative to the first slot in the requested epoch)

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F": [
      0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
      21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
      39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
      57, 58, 59, 60, 61, 62, 63
    ]
  },
  "id": 1
}
```

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getLeaderSchedule",
    "params": [
      null,
      {
        "identity": "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F"
      }
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F": [
      0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
      21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
      39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
      57, 58, 59, 60, 61, 62, 63
    ]
  },
  "id": 1
}
```

### getMaxRetransmitSlot

Get the max slot seen from retransmit stage.

#### Kết quả:

- `<u64>` - Slot

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getMaxRetransmitSlot"}
'
```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### getMaxShredInsertSlot

Get the max slot seen from after shred insert.

#### Kết quả:

- `<u64>` - Slot

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getMaxShredInsertSlot"}
'
```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### getMinimumBalanceForRentExemption

Trả về số dư tối thiểu cần thiết để miễn tiền thuê tài khoản.

#### Thông số:

- `<usize>` - độ dài dữ liệu tài khoản
- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

- `<u64>` - số lamport tối thiểu cần thiết trong tài khoản

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getMinimumBalanceForRentExemption", "params":[50]}
'
```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": 500, "id": 1 }
```

### getMultipleAccounts

Trả về thông tin tài khoản danh sách Pubkey

#### Thông số:

- `<array>` - Một mảng Pubkey để truy vấn, dưới dạng chuỗi được mã hóa base-58
- `<object>` - (tùy chọn) Đối tượng cấu hình chứa các trường tùy chọn sau:
  - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - mã hóa cho dữ liệu Tài khoản, "base58" (_chậm_), "base64", "base64+zstd", hoặc "jsonParsed". "base58" is limited to Account data of less than 129 bytes. "base64" sẽ trả về dữ liệu được mã hóa base64 cho dữ liệu Tài khoản ở bất kỳ kích thước nào. "base64 + zstd" nén dữ liệu Tài khoản bằng cách sử dụng [Zstandard](https://facebook.github.io/zstd/) và base64 mã hóa kết quả. Mã hóa "jsonParsed" cố gắng sử dụng trình phân tích cú pháp trạng thái của chương trình cụ thể để trả về dữ liệu trạng thái tài khoản rõ ràng và dễ đọc hơn. Nếu "jsonParsed" được yêu cầu nhưng không tìm thấy trình phân tích cú pháp, trường sẽ trở lại mã hóa "base64", có thể phát hiện được khi `data` trường được nhập `<string>`.
  - (tùy chọn) `dataSlice: <object>` - giới hạn dữ liệu tài khoản trả về bằng cách sử dụng các trường `offset: <usize>` và `length: <usize>`; chỉ khả dụng cho các mã hóa "base58", "base64" hoặc "base64 + zstd".

#### Kết quả:

Kết quả sẽ là một đối tượng JSON RpcResponse có giá trị `value` bằng:

Một mảng của:

- `<null>` - nếu tài khoản tại Pubkey đó không tồn tại
- `<object>` - nếu không, một đối tượng JSON chứa:
  - `lamports: <u64>`, số lượng lamport được chỉ định cho tài khoản này, dưới dạng u64
  - `owner: <string>`, Pubkey được mã hóa base-58 của chương trình mà tài khoản này đã được gán cho
  - `data: <[string, encoding]|object>`, dữ liệu được liên kết với tài khoản, dưới dạng dữ liệu nhị phân được mã hóa hoặc định dạng JSON `{<program>: <state>}`, tùy thuộc vào thông số mã hóa
  - `executable: <bool>`, boolean cho biết tài khoản có chứa chương trình hay không \(và ở chế độ chỉ đọc\)
  - `rentEpoch: <u64>`, kỷ nguyên mà tài khoản này sẽ nợ tiền thuê tiếp theo, là u64

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getMultipleAccounts",
    "params": [
      [
        "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
        "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA"
      ],
      {
        "dataSlice": {
          "offset": 0,
          "length": 0
        }
      }
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": [
      {
        "data": ["AAAAAAEAAAACtzNsyJrW0g==", "base64"],
        "executable": false,
        "lamports": 1000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      },
      {
        "data": ["", "base64"],
        "executable": false,
        "lamports": 5000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      }
    ]
  },
  "id": 1
}
```

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getMultipleAccounts",
    "params": [
      [
        "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
        "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA"
      ],
      {
        "encoding": "base58"
      }
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": [
      {
        "data": [
          "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHRTPuR3oZ1EioKtYGiYxpxMG5vpbZLsbcBYBEmZZcMKaSoGx9JZeAuWf",
          "base58"
        ],
        "executable": false,
        "lamports": 1000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      },
      {
        "data": ["", "base58"],
        "executable": false,
        "lamports": 5000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      }
    ]
  },
  "id": 1
}
```

### getProgramAccounts

Trả về tất cả các tài khoản thuộc sở hữu của chương trình Pubkey được cung cấp

#### Thông số:

- `<string>` - Pubkey của chương trình, dưới dạng chuỗi mã hóa base-58
- `<object>` - (tùy chọn) Đối tượng cấu hình chứa các trường tùy chọn sau:
  - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - mã hóa cho dữ liệu Tài khoản, "base58" (_chậm_), "base64", "base64+zstd", hoặc "jsonParsed". "base58" is limited to Account data of less than 129 bytes. "base64" sẽ trả về dữ liệu được mã hóa base64 cho dữ liệu Tài khoản ở bất kỳ kích thước nào. "base64 + zstd" nén dữ liệu Tài khoản bằng cách sử dụng [Zstandard](https://facebook.github.io/zstd/) và base64 mã hóa kết quả. Mã hóa "jsonParsed" cố gắng sử dụng trình phân tích cú pháp trạng thái của chương trình cụ thể để trả về dữ liệu trạng thái tài khoản rõ ràng và dễ đọc hơn. Nếu "jsonParsed" được yêu cầu nhưng không tìm thấy trình phân tích cú pháp, trường sẽ trở lại mã hóa "base64", có thể phát hiện được khi `data` trường được nhập `<string>`.
  - (tùy chọn) `dataSlice: <object>` - giới hạn dữ liệu tài khoản trả về bằng cách sử dụng các trường `offset: <usize>` và `length: <usize>`; chỉ khả dụng cho các mã hóa "base58", "base64" hoặc "base64 + zstd".
  - (tùy chọn) `filters: <array>` - lọc kết quả bằng cách sử dụng [filter objects](jsonrpc-api.md#filters); tài khoản phải đáp ứng tất cả các tiêu chí lọc để được đưa vào kết quả

##### Bộ lọc:

- `memcmp: <object>` - so sánh một loạt byte đã cung cấp với dữ liệu tài khoản chương trình tại một khoảng chênh lệch cụ thể. Trường:

  - `offset: <usize>` - bù vào dữ liệu tài khoản chương trình để bắt đầu so sánh
  - `bytes: <string>` - data to match, as base-58 encoded string and limited to less than 129 bytes

- `dataSize: <u64>` - so sánh độ dài dữ liệu tài khoản chương trình với kích thước dữ liệu được cung cấp

#### Kết quả:

Trường kết quả sẽ là một mảng các đối tượng JSON, sẽ chứa:

- `pubkey: <string>` - tài khoản Pubkey dưới dạng chuỗi mã hóa base-58
- `account: <object>` - một đối tượng JSON, với các trường con sau:
  - `lamports: <u64>`, số lượng lamport được chỉ định cho tài khoản này, dưới dạng u64
  - `owner: <string>`, Pubkey được mã hóa base-58 của chương trình mà tài khoản này đã được chỉ định cho `data: <[string,encoding]|object>`, dữ liệu được liên kết với tài khoản, dưới dạng dữ liệu nhị phân được mã hóa hoặc định dạng JSON `{<program>: <state>}`, tùy thuộc vào thông số mã hóa
  - `executable: <bool>`, boolean cho biết tài khoản có chứa chương trình hay không \(và ở chế độ chỉ đọc\)
  - `rentEpoch: <u64>`, kỷ nguyên mà tài khoản này sẽ nợ tiền thuê tiếp theo, là u64

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getProgramAccounts", "params":["4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T"]}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "account": {
        "data": "2R9jLfiAQ9bgdcw6h8s44439",
        "executable": false,
        "lamports": 15298080,
        "owner": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
        "rentEpoch": 28
      },
      "pubkey": "CxELquR1gPP8wHe33gZ4QxqGB3sZ9RSwsJ2KshVewkFY"
    }
  ],
  "id": 1
}
```

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getProgramAccounts",
    "params": [
      "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
      {
        "filters": [
          {
            "dataSize": 17
          },
          {
            "memcmp": {
              "offset": 4,
              "bytes": "3Mc6vR"
            }
          }
        ]
      }
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "account": {
        "data": "2R9jLfiAQ9bgdcw6h8s44439",
        "executable": false,
        "lamports": 15298080,
        "owner": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
        "rentEpoch": 28
      },
      "pubkey": "CxELquR1gPP8wHe33gZ4QxqGB3sZ9RSwsJ2KshVewkFY"
    }
  ],
  "id": 1
}
```

### getRecentBlockhash

Trả về hàm băm khối gần đây từ sổ cái và biểu phí có thể được sử dụng để tính toán chi phí gửi một giao dịch bằng cách sử dụng nó.

#### Thông số:

- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

Một RpcResponse chứa một đối tượng JSON bao gồm một chuỗi khối và đối tượng FeeCalculator JSON.

- `RpcResponse<object>` - Đối tượng JSON RpcResponse với trường `value` được đặt thành đối tượng JSON bao gồm:
- `blockhash: <string>` - một Hash dưới dạng chuỗi được mã hóa base-58
- `feeCalculator: <object>` - FeeCalculator, biểu phí cho hàm băm khối này

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getRecentBlockhash"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "blockhash": "CSymwgTNX1j3E4qhKfJAUE41nBWEwXufoYryPbkde5RR",
      "feeCalculator": {
        "lamportsPerSignature": 5000
      }
    }
  },
  "id": 1
}
```

### getRecentPerformanceSamples

Trả về danh sách các mẫu hiệu suất gần đây, theo thứ tự slot ngược lại. Các mẫu hiệu suất được lấy sau mỗi 60 giây và bao gồm số lượng giao dịch và slot xảy ra trong một khoảng thời gian nhất định.

#### Thông số:

- `limit: <usize>` - (tùy chọn) số lượng mẫu để trả lại (tối đa 720)

#### Kết quả:

Một mảng của:

- `RpcPerfSample<object>`
  - `slot: <u64>` - Slot mà mẫu được lấy tại
  - `numTransactions: <u64>` - Số lượng giao dịch trong mẫu
  - `numSlots: <u64>` - Số lượng slot trong mẫu
  - `samplePeriodSecs: <u16>` - Số giây trong cửa sổ mẫu

#### Ví dụ:

Yêu cầu:

```bash
// Request
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getRecentPerformanceSamples", "params": [4]}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "numSlots": 126,
      "numTransactions": 126,
      "samplePeriodSecs": 60,
      "slot": 348125
    },
    {
      "numSlots": 126,
      "numTransactions": 126,
      "samplePeriodSecs": 60,
      "slot": 347999
    },
    {
      "numSlots": 125,
      "numTransactions": 125,
      "samplePeriodSecs": 60,
      "slot": 347873
    },
    {
      "numSlots": 125,
      "numTransactions": 125,
      "samplePeriodSecs": 60,
      "slot": 347748
    }
  ],
  "id": 1
}
```

### getSnapshotSlot

Trả về slot cao nhất mà node có ảnh chụp nhanh

#### Thông số:

Không có

#### Kết quả:

- `<u64>` - Slot chụp nhanh

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSnapshotSlot"}
'
```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": 100, "id": 1 }
```

Kết quả khi node không có ảnh chụp nhanh:

```json
{
  "jsonrpc": "2.0",
  "error": { "code": -32008, "message": "No snapshot" },
  "id": 1
}
```

### getSignaturesForAddress

Trả lại chữ ký đã xác nhận cho các giao dịch liên quan đến một địa chỉ ngược thời gian từ chữ ký được cung cấp hoặc khối được xác nhận gần đây nhất

#### Thông số:

- `<string>` - địa chỉ tài khoản dưới dạng chuỗi được mã hóa base-58
- `<object>` (tùy chọn) Đối tượng cấu hình chứa trường sau:
  - `limit: <number>` - (tùy chọn) chữ ký giao dịch tối đa để trả về (từ 1 đến 1,000, mặc định: 1,000).
  - `before: <string>` - (tùy chọn) bắt đầu tìm kiếm ngược từ chữ ký giao dịch này. Nếu không được cung cấp, tìm kiếm sẽ bắt đầu từ đầu của khối được xác nhận tối đa cao nhất.
  - `until: <string>` - (tùy chọn) tìm kiếm cho đến khi có được chữ ký cho giao dịch này, nếu được tìm thấy trước khi đạt đến giới hạn.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Kết quả:

Trường kết quả sẽ là một mảng thông tin chữ ký giao dịch, được sắp xếp theo thứ tự từ giao dịch mới nhất đến cũ nhất:

- `<object>`
  - `signature: <string>` - chữ ký giao dịch dưới dạng chuỗi được mã hóa base-58
  - `slot: <u64>` - Slot chứa khối với giao dịch
  - `err: <object | null>` - Error nếu giao dịch không thành công, null nếu giao dịch thành công. [Các định nghĩa của TransactionError](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `memo: <string |null>` - Bản ghi nhớ được liên kết với giao dịch, null nếu không có bản ghi nhớ nào
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when transaction was processed. null if not available.

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [
      "Vote111111111111111111111111111111111111111",
      {
        "limit": 1
      }
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
      "slot": 114,
      "blockTime": null
    }
  ],
  "id": 1
}
```

### getSignatureStatuses

Trả về trạng thái của danh sách các chữ ký. Trừ khi `searchTransactionHistory` thông số cấu hình được bao gồm, chỉ phương thức này tìm kiếm bộ nhớ cache trạng thái gần đây của chữ ký, giữ lại các trạng thái cho tất cả các slot hoạt động cộng với `MAX_RECENT_BLOCKHASHES` các slot đã được root.

#### Thông số:

- `<array>` Một loạt các chữ ký giao dịch để xác nhận, dưới dạng các chuỗi được mã hóa base-58
- `<object>` (tùy chọn) Đối tượng cấu hình chứa trường sau:
  - `searchTransactionHistory: <bool>` nếu đúng, một node Solana sẽ tìm kiếm bộ nhớ đệm sổ cái của nó để tìm bất kỳ chữ ký nào không được tìm thấy trong bộ nhớ đệm trạng thái gần đây

#### Kết quả:

Một RpcResponse chứa một đối tượng JSON bao gồm một mảng các đối tượng TransactionStatus.

- `RpcResponse<object>` - Đối tượng RpcResponse JSON với trường `value`

Một mảng của:

- `<null>` - Giao dịch không xác định
- `<object>`
  - `slot: <u64>` - slot giao dịch đã được xử lý
  - `confirmations: <usize | null>` - Số khối kể từ khi xác nhận chữ ký, rỗng nếu được root, cũng như được hoàn thiện bởi một phần lớn của cụm
  - `err: <object | null>` - Error nếu giao dịch không thành công, null nếu giao dịch thành công. [Các định nghĩa của TransactionError](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `confirmationStatus: <string | null>` - Trạng thái xác nhận cụm của giao dịch; là `processed`, `confirmed`, hoặc `finalized`. Xem [Cam kết](jsonrpc-api.md#configuring-state-commitment) để biết thêm về xác nhận lạc quan.
  - DEPRECATED: `status: <object>` - Trạng thái giao dịch
    - `"Ok": <null>` - Giao dịch thành công
    - `"Err": <ERR>` - Giao dịch không thành công với TransactionError

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignatureStatuses",
    "params": [
      [
        "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW",
        "5j7s6NiJS3JAkvgkoc18WVAsiSaci2pxB2A6ueCJP4tprA2TFg9wSyTLeYouxPBJEMzJinENTkpA52YStRW5Dia7"
      ]
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 82
    },
    "value": [
      {
        "slot": 72,
        "confirmations": 10,
        "err": null,
        "status": {
          "Ok": null
        },
        "confirmationStatus": "confirmed"
      },
      null
    ]
  },
  "id": 1
}
```

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignatureStatuses",
    "params": [
      [
        "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW"
      ],
      {
        "searchTransactionHistory": true
      }
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 82
    },
    "value": [
      {
        "slot": 48,
        "confirmations": null,
        "err": null,
        "status": {
          "Ok": null
        },
        "confirmationStatus": "finalized"
      },
      null
    ]
  },
  "id": 1
}
```

### getSlot

Trả về slot hiện tại mà node đang xử lý

#### Thông số:

- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

- `<u64>` - Slot hiện tại

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlot"}
'
```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### getSlotLeader

Trả về slot leader hiện tại

#### Thông số:

- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

- `<string>` - Nhận dạng node Pubkey dưới dạng chuỗi mã hóa base-58

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeader"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": "ENvAW7JScgYq6o4zKZwewtkzzJgDzuJAFxYasvmEQdpS",
  "id": 1
}
```

### getSlotLeaders

Returns the slot leaders for a given slot range

#### Thông số:

- `<u64>` - Start slot, as u64 integer
- `<u64>` - Giới hạn, dưới dạng số nguyên u64

#### Kết quả:

- `<array<string>>` - Node identity public keys as base-58 encoded strings

#### Ví dụ:

If the current slot is #99, query the next 10 leaders with the following request:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeaders", "params":[100, 10]}
'
```

Kết quả:

The first leader returned is the leader for slot #100:

```json
{
  "jsonrpc": "2.0",
  "result": [
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "DWvDTSh3qfn88UoQTEKRV2JnLt5jtJAVoiCo3ivtMwXP",
    "DWvDTSh3qfn88UoQTEKRV2JnLt5jtJAVoiCo3ivtMwXP"
  ],
  "id": 1
}
```

### getStakeActivation

Trả về thông tin kích hoạt kỷ nguyên cho một tài khoản stake

#### Thông số:

- `<string>` - Pubkey của tài khoản cổ phần để truy vấn, dưới dạng chuỗi được mã hóa base-58
- `<object>` - (tùy chọn) Đối tượng cấu hình chứa các trường tùy chọn sau:
  - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)
  - (tùy chọn) `epoch: <u64>` - kỷ nguyên để tính toán chi tiết kích hoạt. Nếu thông số không được cung cấp, mặc định là kỷ nguyên hiện tại.

#### Kết quả:

Kết quả sẽ là một đối tượng JSON với các trường sau:

- `state: <string` - trạng thái kích hoạt của tài khoản stake, một trong số: `active`, `inactive`, `activating`, `deactivating`
- `active: <u64>` - stake hoạt động trong kỷ nguyên
- `inactive: <u64>` - stake không hoạt động trong kỷ nguyên

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getStakeActivation", "params": ["CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT"]}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": { "active": 197717120, "inactive": 0, "state": "active" },
  "id": 1
}
```

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getStakeActivation",
    "params": [
      "CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT",
      {
        "epoch": 4
      }
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "active": 124429280,
    "inactive": 73287840,
    "state": "activating"
  },
  "id": 1
}
```

### getSupply

Trả về thông tin về nguồn cung hiện tại.

#### Thông số:

- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

Kết quả sẽ là một đối tượng RpcResponse JSON với `value` bằng một đối tượng JSON chứa:

- `total: <u64>` - Tổng nguồn cung trong lamport
- `circulating: <u64>` - Đang lưu thông trong lamport
- `nonCirculating: <u64>` - Nguồn cung không lưu thông trong lamport
- `nonCirculatingAccounts: <array>` - một mảng địa chỉ tài khoản của các tài khoản không lưu hành, dưới dạng chuỗi

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getSupply"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "circulating": 16000,
      "nonCirculating": 1000000,
      "nonCirculatingAccounts": [
        "FEy8pTbP5fEoqMV1GdTz83byuA8EKByqYat1PKDgVAq5",
        "9huDUZfxoJ7wGMTffUE7vh1xePqef7gyrLJu9NApncqA",
        "3mi1GmwEE3zo2jmfDuzvjSX9ovRXsDUKHvsntpkhuLJ9",
        "BYxEJTDerkaRWBem3XgnVcdhppktBXa2HbkHPKj2Ui4Z"
      ],
      "total": 1016000
    }
  },
  "id": 1
}
```

### getTokenAccountBalance

Trả về số dư mã thông báo của tài khoản Mã thông báo SPL.

#### Thông số:

- `<string>` - Pubkey của tài khoản mã thông báo để truy vấn, dưới dạng chuỗi được mã hóa base-58
- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

Kết quả sẽ là một đối tượng RpcResponse JSON với `value` bằng một đối tượng JSON chứa:

- `amount: <string>` - số dư tài khoản mã thông báo thô không có số thập phân, đại diện chuỗi của u64
- `decimals: <u8>` - số cơ số 10 ở bên phải của chữ số thập phân
- `uiAmount: <number | null>` - the balance, using mint-prescribed decimals **DEPRECATED**
- `uiAmountString: <string>` - the balance as a string, using mint-prescribed decimals

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenAccountBalance", "params": ["7fUAJdStEuGbc3sM84cKRL6yYaaSstyLSU4ve5oovLS7"]}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "amount": "9864",
      "decimals": 2,
      "uiAmount": 98.64,
      "uiAmountString": "98.64"
    },
    "id": 1
  }
}
```

### getTokenAccountsByDelegate

Trả lại tất cả các tài khoản Mã thông báo SPL bởi Người được phê duyệt.

#### Thông số:

- `<string>` - Pubkey của tài khoản ủy quyền để truy vấn, dưới dạng chuỗi được mã hóa base-58
- `<object>` - Hoặc:
  - `mint: <string>` - Pubkey của mã thông báo Mint để giới hạn tài khoản, dưới dạng chuỗi mã hóa base-58; hoặc là
  - `programId: <string>` - Pubkey của ID chương trình Mã thông báo người sở hữu tài khoản, dưới dạng chuỗi được mã hóa base-58
- `<object>` - (tùy chọn) Đối tượng cấu hình chứa các trường tùy chọn sau:
  - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - mã hóa cho dữ liệu Tài khoản, "base58" (_chậm_), "base64", "base64 + zstd" hoặc "jsonParsed". Mã hóa "jsonParsed" cố gắng sử dụng trình phân tích cú pháp trạng thái của chương trình cụ thể để trả về dữ liệu trạng thái tài khoản rõ ràng và dễ đọc hơn. Nếu "jsonParsed" được yêu cầu nhưng không thể tìm thấy mã hợp lệ cho một tài khoản cụ thể, tài khoản đó sẽ bị lọc ra khỏi kết quả.
  - (tùy chọn) `dataSlice: <object>` - giới hạn dữ liệu tài khoản trả về bằng cách sử dụng các trường `offset: <usize>` và `length: <usize>`; chỉ khả dụng cho các mã hóa "base58", "base64" hoặc "base64 + zstd".

#### Kết quả:

Kết quả sẽ là một đối tượng JSON RpcResponse với `value` bằng với một mảng các đối tượng JSON, sẽ chứa:

- `pubkey: <string>` - tài khoản Pubkey dưới dạng chuỗi mã hóa base-58
- `account: <object>` - một đối tượng JSON, với các trường con sau:
  - `lamports: <u64>`, số lượng lamport được chỉ định cho tài khoản này, dưới dạng u64
  - `owner: <string>`, Pubkey được mã hóa base-58 của chương trình mà tài khoản này đã được gán cho
  - `data: <object>`, Dữ liệu trạng thái mã thông báo được liên kết với tài khoản, dưới dạng dữ liệu nhị phân được mã hóa hoặc ở định dạng JSON `{<program>: <state>}`
  - `executable: <bool>`, boolean cho biết tài khoản có chứa chương trình hay không \(và ở chế độ chỉ đọc\)
  - `rentEpoch: <u64>`, kỷ nguyên mà tài khoản này sẽ nợ tiền thuê tiếp theo, là u64

#### Ví dụ:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTokenAccountsByDelegate",
    "params": [
      "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
      {
        "programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
      },
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "data": {
          "program": "spl-token",
          "parsed": {
            "accountType": "account",
            "info": {
              "tokenAmount": {
                "amount": "1",
                "decimals": 1,
                "uiAmount": 0.1,
                "uiAmountString": "0.1"
              },
              "delegate": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
              "delegatedAmount": 1,
              "isInitialized": true,
              "isNative": false,
              "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E",
              "owner": "CnPoSPKXu7wJqxe59Fs72tkBeALovhsCxYeFwPCQH9TD"
            }
          }
        },
        "executable": false,
        "lamports": 1726080,
        "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        "rentEpoch": 4
      }
    ]
  },
  "id": 1
}
```

### getTokenAccountsByOwner

Trả về tất cả tài khoản Mã thông báo SPL bởi chủ sở hữu mã thông báo.

#### Thông số:

- `<string>` - Pubkey của chủ tài khoản để truy vấn, dưới dạng chuỗi được mã hóa base-58
- `<object>` - Hoặc:
  - `mint: <string>` - Pubkey của mã thông báo Mint để giới hạn tài khoản, dưới dạng chuỗi mã hóa base-58; hoặc là
  - `programId: <string>` - Pubkey của ID chương trình Mã thông báo người sở hữu tài khoản, dưới dạng chuỗi được mã hóa base-58
- `<object>` - (tùy chọn) Đối tượng cấu hình chứa các trường tùy chọn sau:
  - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - mã hóa cho dữ liệu Tài khoản, "base58" (_chậm_), "base64", "base64 + zstd" hoặc "jsonParsed". Mã hóa "jsonParsed" cố gắng sử dụng trình phân tích cú pháp trạng thái của chương trình cụ thể để trả về dữ liệu trạng thái tài khoản rõ ràng và dễ đọc hơn. Nếu "jsonParsed" được yêu cầu nhưng không thể tìm thấy mã hợp lệ cho một tài khoản cụ thể, tài khoản đó sẽ bị lọc ra khỏi kết quả.
  - (tùy chọn) `dataSlice: <object>` - giới hạn dữ liệu tài khoản trả về bằng cách sử dụng các trường `offset: <usize>` và `length: <usize>`; chỉ khả dụng cho các mã hóa "base58", "base64" hoặc "base64 + zstd".

#### Kết quả:

Kết quả sẽ là một đối tượng JSON RpcResponse với `value` bằng với một mảng các đối tượng JSON, sẽ chứa:

- `pubkey: <string>` - tài khoản Pubkey dưới dạng chuỗi mã hóa base-58
- `account: <object>` - một đối tượng JSON, với các trường con sau:
  - `lamports: <u64>`, số lượng lamport được chỉ định cho tài khoản này, dưới dạng u64
  - `owner: <string>`, Pubkey được mã hóa base-58 của chương trình mà tài khoản này đã được gán cho
  - `data: <object>`, Dữ liệu trạng thái mã thông báo được liên kết với tài khoản, dưới dạng dữ liệu nhị phân được mã hóa hoặc ở định dạng JSON `{<program>: <state>}`
  - `executable: <bool>`, boolean cho biết tài khoản có chứa chương trình hay không \(và ở chế độ chỉ đọc\)
  - `rentEpoch: <u64>`, kỷ nguyên mà tài khoản này sẽ nợ tiền thuê tiếp theo, là u64

#### Ví dụ:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTokenAccountsByOwner",
    "params": [
      "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F",
      {
        "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"
      },
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "data": {
          "program": "spl-token",
          "parsed": {
            "accountType": "account",
            "info": {
              "tokenAmount": {
                "amount": "1",
                "decimals": 1,
                "uiAmount": 0.1,
                "uiAmountString": "0.1"
              },
              "delegate": null,
              "delegatedAmount": 1,
              "isInitialized": true,
              "isNative": false,
              "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E",
              "owner": "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F"
            }
          }
        },
        "executable": false,
        "lamports": 1726080,
        "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        "rentEpoch": 4
      }
    ]
  },
  "id": 1
}
```

### getTokenLargestAccounts

Trả về 20 tài khoản lớn nhất của một loại Mã thông báo SPL cụ thể.

#### Thông số:

- `<string>` - Pubkey của mã thông báo Mint để truy vấn, dưới dạng chuỗi mã hóa base-58
- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

Kết quả sẽ là một đối tượng RpcResponse JSON với `value` bằng một đối tượng JSON chứa:

- `address: <string>` - địa chỉ của tài khoản mã thông báo
- `amount: <string>` - số dư tài khoản mã thông báo thô không có số thập phân, đại diện chuỗi của u64
- `decimals: <u8>` - số cơ số 10 ở bên phải của chữ số thập phân
- `uiAmount: <number | null>` - the token account balance, using mint-prescribed decimals **DEPRECATED**
- `uiAmountString: <string>` - the token account balance as a string, using mint-prescribed decimals

#### Ví dụ:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenLargestAccounts", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "address": "FYjHNoFtSQ5uijKrZFyYAxvEr87hsKXkXcxkcmkBAf4r",
        "amount": "771",
        "decimals": 2,
        "uiAmount": 7.71,
        "uiAmountString": "7.71"
      },
      {
        "address": "BnsywxTcaYeNUtzrPxQUvzAWxfzZe3ZLUJ4wMMuLESnu",
        "amount": "229",
        "decimals": 2,
        "uiAmount": 2.29,
        "uiAmountString": "2.29"
      }
    ]
  },
  "id": 1
}
```

### getTokenSupply

Trả về tổng nguồn cung của loại Mã thông báo SPL.

#### Thông số:

- `<string>` - Pubkey của mã thông báo Mint để truy vấn, dưới dạng chuỗi mã hóa base-58
- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

Kết quả sẽ là một đối tượng RpcResponse JSON với `value` bằng một đối tượng JSON chứa:

- `amount: <string>` - tổng nguồn cung mã thông báo thô không có số thập phân, đại diện chuỗi của u64
- `decimals: <u8>` - số cơ số 10 ở bên phải của chữ số thập phân
- `uiAmount: <number | null>` - the total token supply, using mint-prescribed decimals **DEPRECATED**
- `uiAmountString: <string>` - the total token supply as a string, using mint-prescribed decimals

#### Ví dụ:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenSupply", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "amount": "100000",
      "decimals": 2,
      "uiAmount": 1000,
      "uiAmountString": "1000"
    }
  },
  "id": 1
}
```

### getTransaction

Trả về chi tiết giao dịch cho một giao dịch đã xác nhận

#### Thông số:

- `<string>` - chữ ký giao dịch dưới dạng chuỗi được mã hóa base-58
- `<object>` - (tùy chọn) Đối tượng cấu hình chứa các trường tùy chọn sau:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". Nếu tham số không được cung cấp, mã hóa mặc định là "json". Mã hóa "jsonParsed" cố gắng sử dụng trình phân tích cú pháp hướng dẫn của chương trình cụ thể để trả về dữ liệu rõ ràng và dễ đọc hơn trong danh sách `transaction.message.instructions`. Nếu "jsonParsed" được yêu cầu nhưng một phân tích cú pháp không thể được tìm thấy, các hướng dẫn rơi trở lại để mã hóa JSON thường xuyên (`accounts`, `data`, và `programIdIndex`).
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Kết quả:

- `<null>` - nếu giao dịch không được tìm thấy hoặc không được xác nhận
- `<object>` - nếu giao dịch được xác nhận, một đối tượng có các trường sau:
  - `slot: <u64>` - slot mà giao dịch này đã được xử lý
  - `transaction: <object|[string,encoding]>` - [Đối tượng giao dịch](#transaction-structure), ở định dạng JSON hoặc dữ liệu nhị phân được mã hóa, tùy thuộc vào thông số mã hóa
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when the transaction was processed. null nếu không có sẵn
  - `meta: <object | null>` - đối tượng siêu dữ liệu trạng thái giao dịch:
    - `err: <object | null>` - Error nếu giao dịch không thành công, null nếu giao dịch thành công. [Các định nghĩa của TransactionError](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `fee: <u64>` - phí giao dịch này đã được tính, dưới dạng số nguyên u64
    - `preBalances: <array>` - mảng số dư tài khoản u64 từ trước khi giao dịch được xử lý
    - `postBalances: <array>` - mảng số dư tài khoản u64 sau khi giao dịch được xử lý
    - `innerInstructions: <array|undefined>` - Danh sách [các hướng dẫn bên trong](#inner-instructions-structure) hoặc bị bỏ qua nếu ghi hướng dẫn bên trong chưa được bật trong giao dịch này
    - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `logMessages: <array>` - mảng thông báo nhật ký chuỗi hoặc bị bỏ qua nếu tính năng ghi thông báo nhật ký chưa được bật trong giao dịch này
    - DEPRECATED: `status: <object>` - Trạng thái giao dịch
      - `"Ok": <null>` - Giao dịch thành công
      - `"Err": <ERR>` - Giao dịch không thành công với TransactionError

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "json"
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
      "status": {
        "Ok": null
      }
    },
    "slot": 430,
    "transaction": {
      "message": {
        "accountKeys": [
          "3UVYmECPPMZSCqWKfENfuoTv51fTDTWicX9xmBD2euKe",
          "AjozzgE83A3x1sHNUR64hfH7zaEBWeMaFuAN9kQgujrc",
          "SysvarS1otHashes111111111111111111111111111",
          "SysvarC1ock11111111111111111111111111111111",
          "Vote111111111111111111111111111111111111111"
        ],
        "header": {
          "numReadonlySignedAccounts": 0,
          "numReadonlyUnsignedAccounts": 3,
          "numRequiredSignatures": 1
        },
        "instructions": [
          {
            "accounts": [1, 2, 3, 0],
            "data": "37u9WtQpcm6ULa3WRQHmj49EPs4if7o9f1jSRVZpm2dvihR9C8jY4NqEwXUbLwx15HBSNcP1",
            "programIdIndex": 4
          }
        ],
        "recentBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B"
      },
      "signatures": [
        "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv"
      ]
    }
  },
  "blockTime": null,
  "id": 1
}
```

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "base64"
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
      "status": {
        "Ok": null
      }
    },
    "slot": 430,
    "transaction": [
      "AVj7dxHlQ9IrvdYVIjuiRFs1jLaDMHixgrv+qtHBwz51L4/ImLZhszwiyEJDIp7xeBSpm/TX5B7mYzxa+fPOMw0BAAMFJMJVqLw+hJYheizSoYlLm53KzgT82cDVmazarqQKG2GQsLgiqktA+a+FDR4/7xnDX7rsusMwryYVUdixfz1B1Qan1RcZLwqvxvJl4/t3zHragsUp0L47E24tAFUgAAAABqfVFxjHdMkoVmOYaR1etoteuKObS21cc1VbIQAAAAAHYUgdNXR0u3xNdiTr072z2DVec9EQQ/wNo1OAAAAAAAtxOUhPBp2WSjUNJEgfvy70BbxI00fZyEPvFHNfxrtEAQQEAQIDADUCAAAAAQAAAAAAAACtAQAAAAAAAAdUE18R96XTJCe+YfRfUp6WP+YKCy/72ucOL8AoBFSpAA==",
      "base64"
    ]
  },
  "id": 1
}
```

### getTransactionCount

Trả về số lượng Giao dịch hiện tại từ sổ cái

#### Thông số:

- `<object>` - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Kết quả:

- `<u64>` - đếm

#### Ví dụ:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}
'

```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": 268, "id": 1 }
```

### getVersion

Trả về các phiên bản solana hiện tại đang chạy trên node

#### Thông số:

Không có

#### Kết quả:

Trường kết quả sẽ là một đối tượng JSON với các trường sau:

- `solana-core`, phiên bản phần mềm của solana-core
- `feature-set`, mã định danh duy nhất của bộ mã định danh duy nhất của bộ tính năng của phần mềm hiện tại

#### Ví dụ:

Yêu cầu:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVersion"}
'
```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": { "solana-core": "1.7.0" }, "id": 1 }
```

### getVoteAccounts

Returns the account info and associated stake for all the voting accounts in the current bank.

#### Parameters:

- `<object>` - (optional) Configuration object containing the following field:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `votePubkey: <string>` - Only return results for this validator vote address (base-58 encoded)

#### Results:

The result field will be a JSON object of `current` and `delinquent` accounts, each containing an array of JSON objects with the following sub fields:

- `votePubkey: <string>` - Vote account address, as base-58 encoded string
- `nodePubkey: <string>` - Validator identity, as base-58 encoded string
- `activatedStake: <u64>` - the stake, in lamports, delegated to this vote account and active in this epoch
- `epochVoteAccount: <bool>` - bool, whether the vote account is staked for this epoch
- `commission: <number>`, percentage (0-100) of rewards payout owed to the vote account
- `lastVote: <u64>` - Most recent slot voted on by this vote account
- `epochCredits: <array>` - History of how many credits earned by the end of each epoch, as an array of arrays containing: `[epoch, credits, previousCredits]`

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "current": [
      {
        "commission": 0,
        "epochVoteAccount": true,
        "epochCredits": [
          [1, 64, 0],
          [2, 192, 64]
        ],
        "nodePubkey": "B97CCUW3AEZFGy6uUg6zUdnNYvnVq5VG8PUtb2HayTDD",
        "lastVote": 147,
        "activatedStake": 42,
        "votePubkey": "3ZT31jkAGhUaw8jsy4bTknwBMP8i4Eueh52By4zXcsVw"
      }
    ],
    "delinquent": [
      {
        "commission": 127,
        "epochVoteAccount": false,
        "epochCredits": [],
        "nodePubkey": "6ZPxeQaDo4bkZLRsdNrCzchNQr5LN9QMc9sipXv9Kw8f",
        "lastVote": 0,
        "activatedStake": 0,
        "votePubkey": "CmgCk4aMS7KW1SHX3s9K5tBJ6Yng2LBaC8MFov4wx9sm"
      }
    ]
  },
  "id": 1
}
```

#### Example: Restrict results to a single validator vote account

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getVoteAccounts",
    "params": [
      {
        "votePubkey": "3ZT31jkAGhUaw8jsy4bTknwBMP8i4Eueh52By4zXcsVw"
      }
    ]
  }
'
```

Kết quả:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "current": [
      {
        "commission": 0,
        "epochVoteAccount": true,
        "epochCredits": [
          [1, 64, 0],
          [2, 192, 64]
        ],
        "nodePubkey": "B97CCUW3AEZFGy6uUg6zUdnNYvnVq5VG8PUtb2HayTDD",
        "lastVote": 147,
        "activatedStake": 42,
        "votePubkey": "3ZT31jkAGhUaw8jsy4bTknwBMP8i4Eueh52By4zXcsVw"
      }
    ],
    "delinquent": []
  },
  "id": 1
}
```

### minimumLedgerSlot

Returns the lowest slot that the node has information about in its ledger. This value may increase over time if the node is configured to purge older ledger data

#### Parameters:

Không có

#### Results:

- `u64` - Minimum ledger slot

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"minimumLedgerSlot"}
'

```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### requestAirdrop

Requests an airdrop of lamports to a Pubkey

#### Parameters:

- `<string>` - Pubkey of account to receive lamports, as base-58 encoded string
- `<integer>` - lamports, as a u64
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) (used for retrieving blockhash and verifying airdrop success)

#### Results:

- `<string>` - Transaction Signature of airdrop, as base-58 encoded string

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"requestAirdrop", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri", 1000000000]}
'

```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW",
  "id": 1
}
```

### sendTransaction

Submits a signed transaction to the cluster for processing.

This method does not alter the transaction in any way; it relays the transaction created by clients to the node as-is.

If the node's rpc service receives the transaction, this method immediately succeeds, without waiting for any confirmations. A successful response from this method does not guarantee the transaction is processed or confirmed by the cluster.

While the rpc service will reasonably retry to submit it, the transaction could be rejected if transaction's `recent_blockhash` expires before it lands.

Use [`getSignatureStatuses`](jsonrpc-api.md#getsignaturestatuses) to ensure a transaction is processed and confirmed.

Before submitting, the following preflight checks are performed:

1. Các chữ ký giao dịch đã được xác minh
2. Giao dịch được mô phỏng dựa trên slot ngân hàng được chỉ định bởi cam kết trước. Nếu không thành công, lỗi sẽ được trả lại. Kiểm tra trước khởi hành có thể bị vô hiệu hóa nếu muốn. Nên ghi rõ cam kết giống nhau và cam kết trước để tránh hành vi gây nhầm lẫn.

The returned signature is the first signature in the transaction, which is used to identify the transaction ([transaction id](../../terminology.md#transanction-id)). This identifier can be easily extracted from the transaction data before submission.

#### Parameters:

- `<string>` - fully-signed Transaction, as encoded string
- `<object>` - (optional) Configuration object containing the following field:
  - `skipPreflight: <bool>` - if true, skip the preflight transaction checks (default: false)
  - `preflightCommitment: <string>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) level to use for preflight (default: `"finalized"`).
  - `encoding: <string>` - (optional) Encoding used for the transaction data. Either `"base58"` (_slow_, **DEPRECATED**), or `"base64"`. (default: `"base58"`).

#### Results:

- `<string>` - First Transaction Signature embedded in the transaction, as base-58 encoded string ([transaction id](../../terminology.md#transanction-id))

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "sendTransaction",
    "params": [
      "4hXTCkRzt9WyecNzV1XPgCDfGAZzQKNxLXgynz5QDuWWPSAZBZSHptvWRL3BjCvzUXRdKvHL2b7yGrRQcWyaqsaBCncVG7BFggS8w9snUts67BSh3EqKpXLUm5UMHfD7ZBe9GhARjbNQMLJ1QD3Spr6oMTBU6EhdB4RD8CP2xUxr2u3d6fos36PD98XS6oX8TQjLpsMwncs5DAMiD4nNnR8NBfyghGCWvCVifVwvA8B8TJxE1aiyiv2L429BCWfyzAme5sZW8rDb14NeCQHhZbtNqfXhcp2tAnaAT"
    ]
  }
'

```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": "2id3YC2jK9G5Wo2phDx4gJVAew8DcY5NAojnVuao8rkxwPYPe8cSwE5GzhEgJA2y8fVjDEo6iR6ykBvDxrTQrtpb",
  "id": 1
}
```

### simulateTransaction

Simulate sending a transaction

#### Parameters:

- `<string>` - Transaction, as an encoded string. The transaction must have a valid blockhash, but is not required to be signed.
- `<object>` - (optional) Configuration object containing the following field:
  - `sigVerify: <bool>` - if true the transaction signatures will be verified (default: false)
  - `commitment: <string>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) level to simulate the transaction at (default: `"finalized"`).
  - `encoding: <string>` - (optional) Encoding used for the transaction data. Either `"base58"` (_slow_, **DEPRECATED**), or `"base64"`. (default: `"base58"`).

#### Results:

An RpcResponse containing a TransactionStatus object The result will be an RpcResponse JSON object with `value` set to a JSON object with the following fields:

- `err: <object | string | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
- `logs: <array | null>` - Array of log messages the transaction instructions output during execution, null if simulation failed before the transaction was able to execute (for example due to an invalid blockhash or signature verification failure)

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "simulateTransaction",
    "params": [
      "4hXTCkRzt9WyecNzV1XPgCDfGAZzQKNxLXgynz5QDuWWPSAZBZSHptvWRL3BjCvzUXRdKvHL2b7yGrRQcWyaqsaBCncVG7BFggS8w9snUts67BSh3EqKpXLUm5UMHfD7ZBe9GhARjbNQMLJ1QD3Spr6oMTBU6EhdB4RD8CP2xUxr2u3d6fos36PD98XS6oX8TQjLpsMwncs5DAMiD4nNnR8NBfyghGCWvCVifVwvA8B8TJxE1aiyiv2L429BCWfyzAme5sZW8rDb14NeCQHhZbtNqfXhcp2tAnaAT"
    ]
  }
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 218
    },
    "value": {
      "err": null,
      "logs": [
        "BPF program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri success"
      ]
    }
  },
  "id": 1
}
```

## Subscription Websocket

After connecting to the RPC PubSub websocket at `ws://<ADDRESS>/`:

- Submit subscription requests to the websocket using the methods below
- Multiple subscriptions may be active at once
- Many subscriptions take the optional [`commitment` parameter](jsonrpc-api.md#configuring-state-commitment), defining how finalized a change should be to trigger a notification. For subscriptions, if commitment is unspecified, the default value is `"finalized"`.

### accountSubscribe

Subscribe to an account to receive notifications when the lamports or data for a given account public key changes

#### Parameters:

- `<string>` - account Pubkey, as base-58 encoded string
- `<object>` - (tùy chọn) Đối tượng cấu hình chứa các trường tùy chọn sau:
  - `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to binary encoding, detectable when the `data` field is type `<string>`.

#### Results:

- `<number>` - Subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12",
    {
      "encoding": "base64",
      "commitment": "finalized"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12",
    {
      "encoding": "jsonParsed"
    }
  ]
}
```

Result:

```json
{ "jsonrpc": "2.0", "result": 23784, "id": 1 }
```

#### Notification Format:

Base58 encoding:

```json
{
  "jsonrpc": "2.0",
  "method": "accountNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5199307
      },
      "value": {
        "data": [
          "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR",
          "base58"
        ],
        "executable": false,
        "lamports": 33594,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 635
      }
    },
    "subscription": 23784
  }
}
```

Parsed-JSON encoding:

```json
{
  "jsonrpc": "2.0",
  "method": "accountNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5199307
      },
      "value": {
        "data": {
          "program": "nonce",
          "parsed": {
            "type": "initialized",
            "info": {
              "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
              "blockhash": "LUaQTmM7WbMRiATdMMHaRGakPtCkc2GHtH57STKXs6k",
              "feeCalculator": {
                "lamportsPerSignature": 5000
              }
            }
          }
        },
        "executable": false,
        "lamports": 33594,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 635
      }
    },
    "subscription": 23784
  }
}
```

### accountUnsubscribe

Unsubscribe from account change notifications

#### Parameters:

- `<number>` - id of account Subscription to cancel

#### Results:

- `<bool>` - hủy đăng ký thông báo thành công

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "accountUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### logsSubscribe

Subscribe to transaction logging

#### Parameters:

- `filter: <string>|<object>` - filter criteria for the logs to receive results by account type; currently supported:
  - "all" - subscribe to all transactions except for simple vote transactions
  - "allWithVotes" - subscribe to all transactions including simple vote transactions
  - `{ "mentions": [ <string> ] }` - subscribe to all transactions that mention the provided Pubkey (as base-58 encoded string)
- `<object>` - (tùy chọn) Đối tượng cấu hình chứa các trường tùy chọn sau:
  - (tùy chọn) [Cam kết](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<integer>` - Id Đăng ký \(cần thiết để hủy đăng ký\)

#### Example:

Request:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "logsSubscribe",
  "params": [
    {
      "mentions": [ "11111111111111111111111111111111" ]
    },
    {
      "commitment": "finalized"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "logsSubscribe",
  "params": [ "all" ]
}
```

Result:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### Notification Format:

Base58 encoding:

```json
{
  "jsonrpc": "2.0",
  "method": "logsNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
        "err": null,
        "logs": [
          "BPF program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri success"
        ]
      }
    },
    "subscription": 24040
  }
}
```

### logsUnsubscribe

Unsubscribe from transaction logging

#### Parameters:

- `<integer>` - id of subscription to cancel

#### Results:

- `<bool>` - hủy đăng ký thông báo thành công

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "logsUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### programSubscribe

Subscribe to a program to receive notifications when the lamports or data for a given account owned by the program changes

#### Parameters:

- `<string>` - program_id Pubkey, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to base64 encoding, detectable when the `data` field is type `<string>`.
  - (optional) `filters: <array>` - filter results using various [filter objects](jsonrpc-api.md#filters); account must meet all filter criteria to be included in results

#### Results:

- `<integer>` - Subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "base64",
      "commitment": "finalized"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "jsonParsed"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "base64",
      "filters": [
        {
          "dataSize": 80
        }
      ]
    }
  ]
}
```

Result:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### Notification Format:

Base58 encoding:

```json
{
  "jsonrpc": "2.0",
  "method": "programNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "pubkey": "H4vnBqifaSACnKa7acsxstsY1iV1bvJNxsCY7enrd1hq",
        "account": {
          "data": [
            "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR",
            "base58"
          ],
          "executable": false,
          "lamports": 33594,
          "owner": "11111111111111111111111111111111",
          "rentEpoch": 636
        }
      }
    },
    "subscription": 24040
  }
}
```

Parsed-JSON encoding:

```json
{
  "jsonrpc": "2.0",
  "method": "programNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "pubkey": "H4vnBqifaSACnKa7acsxstsY1iV1bvJNxsCY7enrd1hq",
        "account": {
          "data": {
            "program": "nonce",
            "parsed": {
              "type": "initialized",
              "info": {
                "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
                "blockhash": "LUaQTmM7WbMRiATdMMHaRGakPtCkc2GHtH57STKXs6k",
                "feeCalculator": {
                  "lamportsPerSignature": 5000
                }
              }
            }
          },
          "executable": false,
          "lamports": 33594,
          "owner": "11111111111111111111111111111111",
          "rentEpoch": 636
        }
      }
    },
    "subscription": 24040
  }
}
```

### programUnsubscribe

Unsubscribe from program-owned account change notifications

#### Parameters:

- `<integer>` - id of account Subscription to cancel

#### Results:

- `<bool>` - hủy đăng ký thông báo thành công

#### Example:

Yêu cầu:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "programUnsubscribe", "params": [0] }
```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### signatureSubscribe

Subscribe to a transaction signature to receive notification when the transaction is confirmed On `signatureNotification`, the subscription is automatically cancelled

#### Parameters:

- `<string>` - Transaction Signature, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "signatureSubscribe",
  "params": [
    "2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b"
  ]
}

{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "signatureSubscribe",
  "params": [
    "2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b",
    {
      "commitment": "finalized"
    }
  ]
}
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

```bash
{
  "jsonrpc": "2.0",
  "method": "signatureNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5207624
      },
      "value": {
        "err": null
      }
    },
    "subscription": 24006
  }
}
```

### signatureUnsubscribe

Unsubscribe from signature confirmation notification

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "signatureUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### slotSubscribe

Subscribe to receive notification anytime a slot is processed by the validator

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotSubscribe" }
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

```bash
{
  "jsonrpc": "2.0",
  "method": "slotNotification",
  "params": {
    "result": {
      "parent": 75,
      "root": 44,
      "slot": 76
    },
    "subscription": 0
  }
}
```

### slotUnsubscribe

Unsubscribe from slot notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Yêu cầu:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotUnsubscribe", "params": [0] }
```

Kết quả:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### rootSubscribe

Subscribe to receive notification anytime a new root is set by the validator.

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Yêu cầu:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootSubscribe" }
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

The result is the latest root slot number.

```bash
{
  "jsonrpc": "2.0",
  "method": "rootNotification",
  "params": {
    "result": 42,
    "subscription": 0
  }
}
```

### rootUnsubscribe

Unsubscribe from root notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### voteSubscribe - Unstable, disabled by default

**This subscription is unstable and only available if the validator was started with the `--rpc-pubsub-enable-vote-subscription` flag. The format of this subscription may change in the future**

Subscribe to receive notification anytime a new vote is observed in gossip. These votes are pre-consensus therefore there is no guarantee these votes will enter the ledger.

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteSubscribe" }
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

The result is the latest vote, containing its hash, a list of voted slots, and an optional timestamp.

```json
{
  "jsonrpc": "2.0",
  "method": "voteNotification",
  "params": {
    "result": {
      "hash": "8Rshv2oMkPu5E4opXTRyuyBeZBqQ4S477VG26wUTFxUM",
      "slots": [1, 2],
      "timestamp": null
    },
    "subscription": 0
  }
}
```

### voteUnsubscribe

Unsubscribe from vote notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteUnsubscribe", "params": [0] }
```

Response:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

## JSON RPC API Deprecated Methods

### getConfirmedBlock

**DEPRECATED: Please use [getBlock](jsonrpc-api.md#getblock) instead** This method is expected to be removed in solana-core v1.8

Returns identity and transaction information about a confirmed block in the ledger

#### Parameters:

- `<u64>` - slot, as u64 integer
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". If parameter not provided, the default encoding is "json". "jsonParsed" encoding attempts to use program-specific instruction parsers to return more human-readable and explicit data in the `transaction.message.instructions` list. If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).
  - (optional) `transactionDetails: <string>` - level of transaction detail to return, either "full", "signatures", or "none". If parameter not provided, the default detail level is "full".
  - (optional) `rewards: bool` - whether to populate the `rewards` array. If parameter not provided, the default includes rewards.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an object with the following fields:

- `<null>` - if specified block is not confirmed
- `<object>` - if block is confirmed, an object with the following fields:
  - `blockhash: <string>` - the blockhash of this block, as base-58 encoded string
  - `previousBlockhash: <string>` - the blockhash of this block's parent, as base-58 encoded string; if the parent block is not available due to ledger cleanup, this field will return "11111111111111111111111111111111"
  - `parentSlot: <u64>` - the slot index of this block's parent
  - `transactions: <array>` - present if "full" transaction details are requested; an array of JSON objects containing:
    - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
    - `meta: <object>` - transaction status metadata object, containing `null` or:
      - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - `fee: <u64>` - fee this transaction was charged, as u64 integer
      - `preBalances: <array>` - array of u64 account balances from before the transaction was processed
      - `postBalances: <array>` - array of u64 account balances after the transaction was processed
      - `innerInstructions: <array|undefined>` - List of [inner instructions](#inner-instructions-structure) or omitted if inner instruction recording was not yet enabled during this transaction
      - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `logMessages: <array>` - array of string log messages or omitted if log message recording was not yet enabled during this transaction
      - DEPRECATED: `status: <object>` - Transaction status
        - `"Ok": <null>` - Transaction was successful
        - `"Err": <ERR>` - Transaction failed with TransactionError
  - `signatures: <array>` - present if "signatures" are requested for transaction details; an array of signatures strings, corresponding to the transaction order in the block
  - `rewards: <array>` - present if rewards are requested; an array of JSON objects containing:
    - `pubkey: <string>` - The public key, as base-58 encoded string, of the account that received the reward
    - `lamports: <i64>`- number of reward lamports credited or debited by the account, as a i64
    - `postBalance: <u64>` - account balance in lamports after the reward was applied
    - `rewardType: <string|undefined>` - type of reward: "fee", "rent", "voting", "staking"
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch). null if not available

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, {"encoding": "json","transactionDetails":"full","rewards":false}]}
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
          "status": {
            "Ok": null
          }
        },
        "transaction": {
          "message": {
            "accountKeys": [
              "3UVYmECPPMZSCqWKfENfuoTv51fTDTWicX9xmBD2euKe",
              "AjozzgE83A3x1sHNUR64hfH7zaEBWeMaFuAN9kQgujrc",
              "SysvarS1otHashes111111111111111111111111111",
              "SysvarC1ock11111111111111111111111111111111",
              "Vote111111111111111111111111111111111111111"
            ],
            "header": {
              "numReadonlySignedAccounts": 0,
              "numReadonlyUnsignedAccounts": 3,
              "numRequiredSignatures": 1
            },
            "instructions": [
              {
                "accounts": [1, 2, 3, 0],
                "data": "37u9WtQpcm6ULa3WRQHmj49EPs4if7o9f1jSRVZpm2dvihR9C8jY4NqEwXUbLwx15HBSNcP1",
                "programIdIndex": 4
              }
            ],
            "recentBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B"
          },
          "signatures": [
            "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv"
          ]
        }
      }
    ]
  },
  "id": 1
}
```

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "base64"]}
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
          "status": {
            "Ok": null
          }
        },
        "transaction": [
          "AVj7dxHlQ9IrvdYVIjuiRFs1jLaDMHixgrv+qtHBwz51L4/ImLZhszwiyEJDIp7xeBSpm/TX5B7mYzxa+fPOMw0BAAMFJMJVqLw+hJYheizSoYlLm53KzgT82cDVmazarqQKG2GQsLgiqktA+a+FDR4/7xnDX7rsusMwryYVUdixfz1B1Qan1RcZLwqvxvJl4/t3zHragsUp0L47E24tAFUgAAAABqfVFxjHdMkoVmOYaR1etoteuKObS21cc1VbIQAAAAAHYUgdNXR0u3xNdiTr072z2DVec9EQQ/wNo1OAAAAAAAtxOUhPBp2WSjUNJEgfvy70BbxI00fZyEPvFHNfxrtEAQQEAQIDADUCAAAAAQAAAAAAAACtAQAAAAAAAAdUE18R96XTJCe+YfRfUp6WP+YKCy/72ucOL8AoBFSpAA==",
          "base64"
        ]
      }
    ]
  },
  "id": 1
}
```

For more details on returned data: [Transaction Structure](jsonrpc-api.md#transactionstructure) [Inner Instructions Structure](jsonrpc-api.md#innerinstructionsstructure) [Token Balances Structure](jsonrpc-api.md#tokenbalancesstructure)

### getConfirmedBlocks

**DEPRECATED: Please use [getBlocks](jsonrpc-api.md#getblocks) instead** This method is expected to be removed in solana-core v1.8

Returns a list of confirmed blocks between two slots

#### Parameters:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - (optional) end_slot, as u64 integer
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an array of u64 integers listing confirmed blocks between `start_slot` and either `end_slot`, if provided, or latest confirmed block, inclusive. Max range allowed is 500,000 slots.

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5, 10]}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7, 8, 9, 10], "id": 1 }
```

### getConfirmedBlocksWithLimit

**DEPRECATED: Please use [getBlocksWithLimit](jsonrpc-api.md#getblockswithlimit) instead** This method is expected to be removed in solana-core v1.8

Returns a list of confirmed blocks starting at the given slot

#### Parameters:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - limit, as u64 integer
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an array of u64 integers listing confirmed blocks starting at `start_slot` for up to `limit` blocks, inclusive.

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocksWithLimit","params":[5, 3]}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7], "id": 1 }
```

### getConfirmedSignaturesForAddress2

**DEPRECATED: Please use [getSignaturesForAddress](jsonrpc-api.md#getsignaturesforaddress) instead** This method is expected to be removed in solana-core v1.8

Returns confirmed signatures for transactions involving an address backwards in time from the provided signature or most recent confirmed block

#### Parameters:

- `<string>` - account address as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following fields:
  - `limit: <number>` - (optional) maximum transaction signatures to return (between 1 and 1,000, default: 1,000).
  - `before: <string>` - (optional) start searching backwards from this transaction signature. If not provided the search starts from the top of the highest max confirmed block.
  - `until: <string>` - (optional) search until this transaction signature, if found before limit reached.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an array of transaction signature information, ordered from newest to oldest transaction:

- `<object>`
  - `signature: <string>` - transaction signature as base-58 encoded string
  - `slot: <u64>` - The slot that contains the block with the transaction
  - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `memo: <string |null>` - Memo associated with the transaction, null if no memo is present
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when transaction was processed. null if not available.

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedSignaturesForAddress2",
    "params": [
      "Vote111111111111111111111111111111111111111",
      {
        "limit": 1
      }
    ]
  }
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
      "slot": 114,
      "blockTime": null
    }
  ],
  "id": 1
}
```

### getConfirmedTransaction

**DEPRECATED: Please use [getTransaction](jsonrpc-api.md#gettransaction) instead** This method is expected to be removed in solana-core v1.8

Returns transaction details for a confirmed transaction

#### Parameters:

- `<string>` - transaction signature as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". If parameter not provided, the default encoding is "json". "jsonParsed" encoding attempts to use program-specific instruction parsers to return more human-readable and explicit data in the `transaction.message.instructions` list. If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

- `<null>` - if transaction is not found or not confirmed
- `<object>` - if transaction is confirmed, an object with the following fields:
  - `slot: <u64>` - the slot this transaction was processed in
  - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when the transaction was processed. null if not available
  - `meta: <object | null>` - transaction status metadata object:
    - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `fee: <u64>` - fee this transaction was charged, as u64 integer
    - `preBalances: <array>` - array of u64 account balances from before the transaction was processed
    - `postBalances: <array>` - array of u64 account balances after the transaction was processed
    - `innerInstructions: <array|undefined>` - List of [inner instructions](#inner-instructions-structure) or omitted if inner instruction recording was not yet enabled during this transaction
    - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `logMessages: <array>` - array of string log messages or omitted if log message recording was not yet enabled during this transaction
    - DEPRECATED: `status: <object>` - Transaction status
      - `"Ok": <null>` - Transaction was successful
      - `"Err": <ERR>` - Transaction failed with TransactionError

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "json"
    ]
  }
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
      "status": {
        "Ok": null
      }
    },
    "slot": 430,
    "transaction": {
      "message": {
        "accountKeys": [
          "3UVYmECPPMZSCqWKfENfuoTv51fTDTWicX9xmBD2euKe",
          "AjozzgE83A3x1sHNUR64hfH7zaEBWeMaFuAN9kQgujrc",
          "SysvarS1otHashes111111111111111111111111111",
          "SysvarC1ock11111111111111111111111111111111",
          "Vote111111111111111111111111111111111111111"
        ],
        "header": {
          "numReadonlySignedAccounts": 0,
          "numReadonlyUnsignedAccounts": 3,
          "numRequiredSignatures": 1
        },
        "instructions": [
          {
            "accounts": [1, 2, 3, 0],
            "data": "37u9WtQpcm6ULa3WRQHmj49EPs4if7o9f1jSRVZpm2dvihR9C8jY4NqEwXUbLwx15HBSNcP1",
            "programIdIndex": 4
          }
        ],
        "recentBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B"
      },
      "signatures": [
        "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv"
      ]
    }
  },
  "blockTime": null,
  "id": 1
}
```

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "base64"
    ]
  }
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
      "status": {
        "Ok": null
      }
    },
    "slot": 430,
    "transaction": [
      "AVj7dxHlQ9IrvdYVIjuiRFs1jLaDMHixgrv+qtHBwz51L4/ImLZhszwiyEJDIp7xeBSpm/TX5B7mYzxa+fPOMw0BAAMFJMJVqLw+hJYheizSoYlLm53KzgT82cDVmazarqQKG2GQsLgiqktA+a+FDR4/7xnDX7rsusMwryYVUdixfz1B1Qan1RcZLwqvxvJl4/t3zHragsUp0L47E24tAFUgAAAABqfVFxjHdMkoVmOYaR1etoteuKObS21cc1VbIQAAAAAHYUgdNXR0u3xNdiTr072z2DVec9EQQ/wNo1OAAAAAAAtxOUhPBp2WSjUNJEgfvy70BbxI00fZyEPvFHNfxrtEAQQEAQIDADUCAAAAAQAAAAAAAACtAQAAAAAAAAdUE18R96XTJCe+YfRfUp6WP+YKCy/72ucOL8AoBFSpAA==",
      "base64"
    ]
  },
  "id": 1
}
```
