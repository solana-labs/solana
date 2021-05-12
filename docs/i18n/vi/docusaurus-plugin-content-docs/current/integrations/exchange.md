---
title: Thêm Solana vào Sàn giao dịch của bạn
---

Hướng dẫn này mổ tả cách thêm mã thông báo SOL của Solana vào sàn giao dịch tiền điện tử của bạn.

## Thiết lập Node

Chúng tôi thực sự khuyên bạn nên thiết lập ít nhất hai node trên máy tính cao cấp/phiên bản đám mây, nâng cấp lên phiên bản mới hơn ngay lập tức và theo dõi các hoạt động dịch vụ bằng công cụ giám sát đi kèm.

Thiết lập này cho phép bạn:

- để có một cổng thông tin đáng tin cậy vào cụm mainnet-beta của Solana để lấy dữ liệu và gửi các giao dịch rút tiền
- có toàn quyền kiểm soát toàn bộ số dữ liệu về lịch sử khối được lưu giữ
- để duy trì tính khả dụng của dịch vụ ngay cả khi một node bị lỗi

Các node Solana yêu cầu sức mạnh tính toán tương đối cao để xử lý các khối nhanh và TPS cao. Đối với các yêu cầu cụ thể, vui lòng xem [khuyến nghị phần cứng](../running-validator/validator-reqs.md).

Để chạy một node api:

1. [Cài đặt bộ công cụ dòng lệnh Solana](../cli/install-solana-cli-tools.md)
2. Khởi động validator với các thông số sau:

```bash
solana-validator \
  --ledger <LEDGER_PATH> \
  --entrypoint <CLUSTER_ENTRYPOINT> \
  --expected-genesis-hash <EXPECTED_GENESIS_HASH> \
  --rpc-port 8899 \
  --no-voting \
  --enable-rpc-transaction-history \
  --limit-ledger-size \
  --trusted-validator <VALIDATOR_ADDRESS> \
  --no-untrusted-rpc
```

Tùy chỉnh `--ledger` cho vị trí lưu trữ ledger mong muốn của bạn và `--rpc-port` tới cổng bạn muốn hiển thị.

Các thông số `--entrypoint` và `--expected-genesis-hash` đều dành riêng cho cụm mà bạn đang tham gia. [Các thông số hiện tại cho Mainnet Beta](../clusters.md#example-solana-validator-command-line-2)

Thông số `--limit-ledger-size` cho phép bạn chỉ định số lượng sổ cái [shreds](../terminology.md#shred) mà node của bạn giữ lại trên ổ đĩa cứng. Nếu không có thông số này, validator sẽ giữ toàn bộ sổ cái cho đến khi nó hết dung lượng ổ đĩa cứng. Giá trị mặc định cố gắng duy trì mức sử dụng ổ đĩa cứng sổ cái dưới 500GB. Có thể yêu cầu mức sử dụng ổ đĩa cứng nhiều hơn hoặc ít hơn bằng cách thêm đối số vào `--limit-ledger-size` nếu muốn. Kiểm tra `solana-validator --help` để biết giá trị giới hạn mặc định được sử dụng bởi `--limit-ledger-size`. Thông tin thêm về việc chọn một giá trị giới hạn tùy chỉnh [có sẵn tại đây](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26).

Chỉ định một hoặc nhiều thông số `--trusted-validator` có thể bảo vệ bạn khỏi một ảnh chụp nhanh độc hại. T[Tìm hiểu thêm về giá trị của việc khởi động với các validator đáng tin cậy](../running-validator/validator-start.md#trusted-validators)

Các thông số tùy chọn cần xem xét:

- `--private-rpc` ngăn cổng RPC của bạn không được xuất bản để các node khác sử dụng
- `--rpc-bind-address` cho phép bạn chỉ định một địa chỉ IP khác để liên kết cổng RPC

### Tự động khởi động lại và giám sát

Chúng tôi khuyên bạn nên cấu hình từng node của mình để tự động khởi động lại khi thoát, để đảm bảo bạn bỏ lỡ càng ít dữ liệu càng tốt. Chạy phần mềm solana như một dịch vụ hệ thống là một lựa chọn tuyệt vời.

Để giám sát, chúng tôi cung cấp [`solana-watchtower`](https://github.com/solana-labs/solana/blob/master/watchtower/README.md) có thể giám sát validator của bạn và phát hiện `solana-validator` quá trình không lành mạnh. Nó có thể được cấu hình trực tiếp để thông báo cho bạn qua Slack, Telegram, Discord hoặc Twillio. Để biết chi tiết, hãy chạy code>solana-watchtower --help</code>.

```bash
solana-watchtower --validator-identity <YOUR VALIDATOR IDENTITY>
```

#### Thông báo phát hành phần mềm mới

Chúng tôi phát hành phần mềm mới thường xuyên (khoảng 1 bản/tuần). Đôi khi các phiên bản mới hơn bao gồm các thay đổi giao thức không tương thích, cần cập nhật phần mềm kịp thời để tránh lỗi trong xử lý các khối.

Thông báo phát hành chính thức của chúng tôi cho tất cả các loại bản phát hành (thông thường và bảo mật) được thông báo qua kênh discord có tên [`#mb-announcement`](https://discord.com/channels/428295358100013066/669406841830244375) (`mb` viết tắt của `mainnet-beta`).

Giống như các validator stake, chúng tôi hy vọng mọi validator do sàn giao dịch điều hành sẽ được cập nhật sớm nhất trong thời gian thuận tiện cho bạn trong vòng một hoặc hai ngày làm việc sau thông báo phát hành bình thường. Đối với các bản phát hành liên quan đến bảo mật, có thể sẽ hành động khẩn cấp hơn. Đối với các bản phát hành liên quan đến bảo mật, có thể cần hành động khẩn cấp hơn.

### Sổ cái liên tục

Theo mặc định, mỗi node của bạn sẽ khởi động từ một ảnh chụp nhanh do một trong những validator đáng tin cậy bạn cung cấp. Ảnh chụp nhanh này phản ánh trạng thái hiện tại của chuỗi, nhưng không chứa lịch sử sổ cái hoàn chỉnh. Nếu một trong các node của bạn thoát ra và khởi động từ một ảnh chụp nhanh mới, có thể có một khoảng trống trong sổ cái trên node đó. Để ngăn chặn sự cố này, hãy thêm thông số `--no-snapshot-fetch` vào lệnh `solana-validator` của bạn để nhận lịch sử dữ liệu sổ cái thay vì ảnh chụp nhanh.

Không chuyển thông số `--no-snapshot-fetch` vào lần khởi động đầu tiên của bạn vì không thể khởi động node từ tất cả các cách từ khối genesis. Thay vào đó, hãy khởi động từ một ảnh chụp nhanh trước và sau đó thêm thông số `--no-snapshot-fetch` để khởi động lại.

Điều quan trọng cần lưu ý là số lượng lịch sử sổ cái có sẵn cho các node của bạn từ phần còn lại của mạng bị giới hạn tại bất kỳ thời điểm nào. Sau khi hoạt động nếu các validator của bạn gặp phải thời gian chết, chúng có thể không bắt kịp mạng và sẽ cần tải xuống ảnh chụp nhanh mới từ validator đáng tin cậy. Khi làm như vậy, các validator của bạn bây giờ sẽ có một khoảng trống trong dữ liệu lịch sử sổ cái của nó mà không thể được lấp đầy.

### Giảm Thiểu Phơi Sáng Cổng validator

Validator yêu cầu các cổng UDP và TCP khác nhau phải mở cho lưu lượng đến từ tất cả các vadidator Solana khác. Mặc dù đây là phương thức hoạt động hiệu quả nhất và rất được khuyến khích nhưng có thể hạn chế, validator chỉ yêu cầu lưu lượng truy cập đến từ một validator Solana khác.

Đầu tiên hãy thêm đối số `--restricted-repair-only-mode`. Điều này sẽ khiến validator hoạt động ở chế độ hạn chế, nơi nó sẽ không nhận được các cú hích từ phần còn lại của các validator và thay vào đó sẽ cần liên tục thăm dò các validator khác cho các khối. Validator sẽ chỉ truyền các gói UDP đến các validator khác bằng cách sử dụng các cổng _Gossip_ và _ServeR_ ("phục vụ sửa chữa")* và chỉ nhận các gói UDP trên các cổng *Gossip* và *Repair\*.</p>

Cổng _Gossip_ là hai chiều và cho phép validator của bạn tiếp tục liên lạc với phần còn lại của cụm. Validator của bạn truyền trên _ServeR_ để thực hiện các yêu cầu sửa chữa nhằm lấy các khối mới từ phần còn lại của mạng, vì Turbine hiện đã bị vô hiệu hóa. Validator của bạn sau đó sẽ nhận được phản hồi sửa chữa trên cổng _Sửa chữa_ từ các validator khác.

Để hạn chế hơn nữa validator chỉ yêu cầu khối từ một hoặc nhiều validator, trước tiên hãy xác định pubkey nhận dạng cho validator đó và thêm đối số `--gossip-pull-validator PUBKEY --repair-validator PUBKEY` cho mỗi PUBKEY. Điều này sẽ khiến validator của bạn tiêu hao tài nguyên trên mỗi validator mà bạn thêm vào, vì vậy hãy thực hiện điều này một cách tiết kiệm và chỉ sau khi tham khảo ý kiến ​​của validator mục tiêu.

Validator của bạn bây giờ chỉ nên giao tiếp với các validator được liệt kê cụ thể và trên các cổng _Gossip_, _Repair_ và _ServeR_.

## Thiết lập Tài khoản Tiền gửi

Tài khoản Solana không yêu cầu bất kỳ khởi tạo nào trên chuỗi; khi nó có chứa một ít SOL thì chúng sẽ tồn tại. Để thiết lập tài khoản tiền gửi cho sàn giao dịch của bạn, chỉ cần tạo keypair Solana bằng bất kỳ [công cụ ví](../wallet-guide/cli.md) nào của chúng tôi.

Chúng tôi khuyên bạn nên sử dụng một tài khoản tiền gửi duy nhất cho mỗi người dùng của bạn.

Tài khoản Solana được tính [tiền thuê](developing/programming-model/accounts.md#rent) khi tạo và một lần cho mỗi kỷ nguyên, nhưng có thể được miễn tiền thuê nếu chúng có giá trị thuê 2 năm trong SOL. Để tìm hiểu số dư tối thiểu để bạn được miễn tiền thuê cho tài khoản tiền gửi của bạn, truy vấn [`getMinimumBalanceForRentExemption`điểm cuối](developing/clients/jsonrpc-api.md#getminimumbalanceforrentexemption):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getMinimumBalanceForRentExemption","params":[0]}' localhost:8899

{"jsonrpc":"2.0","result":890880,"id":1}
```

### Tài khoản Ngoại tuyến

Bạn có thể giữ khóa của một hoặc nhiều tài khoản ngoại tuyến để nó bảo mật cao hơn. Nếu vậy, bạn sẽ cần phải chuyển SOL sang các tài khoản nóng bằng cách sử dụng [phương pháp ngoại tuyến](../offline-signing.md) của chúng tôi.

## Lắng nghe khoản Tiền gửi

Khi người dùng muốn gửi SOL vào sàn giao dịch của bạn, hãy hướng dẫn họ gửi chuyển khoản đến địa chỉ gửi tiền thích hợp.

### Thăm dò ý kiến ​​cho các khối

Để theo dõi tất cả các tài khoản tiền gửi cho sàn giao dịch của bạn, hãy thăm dò ý kiến ​​cho từng khối đã xác nhận và kiểm tra các địa chỉ quan tâm, sử dụng dịch vụ JSON-RPC của node API Solana của bạn.

- Để xác định khối nào có sẵn, hãy gửi [`getConfirmedBlocks` yêu cầu](developing/clients/jsonrpc-api.md#getconfirmedblocks), chuyển khối cuối cùng mà bạn đã xử lý làm tham số slot bắt đầu:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5]}' localhost:8899

{"jsonrpc":"2.0","result":[5,6,8,9,11],"id":1}
```

Không phải mọi slot đều tạo ra một khối, vì vậy có thể có khoảng trống trong chuỗi các số nguyên.

- Đối với mỗi khối, hãy yêu cầu nội dung của nó bằng [`getConfirmedBlock` yêu cầu](developing/clients/jsonrpc-api.md#getconfirmedblock):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[5, "json"]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": {
    "blockhash": "2WcrsKSVANoe6xQHKtCcqNdUpCQPQ3vb6QTgi1dcE2oL",
    "parentSlot": 4,
    "previousBlockhash": "7ZDoGW83nXgP14vnn9XhGSaGjbuLdLWkQAoUQ7pg6qDZ",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "postBalances": [
            2033973061360,
            218099990000,
            42000000003
          ],
          "preBalances": [
            2044973066360,
            207099990000,
            42000000003
          ],
          "status": {
            "Ok": null
          }
        },
        "transaction": {
          "message": {
            "accountKeys": [
              "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
              "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
              "11111111111111111111111111111111"
            ],
            "header": {
              "numReadonlySignedAccounts": 0,
              "numReadonlyUnsignedAccounts": 1,
              "numRequiredSignatures": 1
            },
            "instructions": [
              {
                "accounts": [
                  0,
                  1
                ],
                "data": "3Bxs3zyH82bhpB8j",
                "programIdIndex": 2
              }
            ],
            "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
          },
          "signatures": [
            "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
          ]
        }
      }
    ]
  },
  "id": 1
}
```

Các trường `preBalances` và `postBalances` cho phép bạn theo dõi các thay đổi số dư trong mọi tài khoản mà không cần phải phân tích cú pháp toàn bộ giao dịch. Họ liệt kê số dư đầu kỳ và số dư cuối kỳ của mỗi tài khoản [lamport](../terminology.md#lamport), được lập chỉ mục vào danh sách `accountKeys`. Đối với ví dụ, nếu địa chỉ gửi tiền nếu lãi suất là `47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi`, giao dịch này đại diện cho chuyển khoản 218099990000 - 207099990000 = 11000000000 lamports = 11 SOL

Nếu bạn cần thêm thông tin về giao dịch hoặc các chi tiết cụ thể khác, bạn có thể yêu cầu khối từ RPC ở định dạng nhị phân và phân tích cú pháp nó bằng cách sử dụng [Rust SDK](https://github.com/solana-labs/solana) hoặc [Javascript SDK](https://github.com/solana-labs/solana-web3.js).

### Lịch sử Địa chỉ

Bạn cũng có thể truy vấn lịch sử giao dịch của một địa chỉ cụ thể. Đây _không phải_ là một phương pháp khả thi để theo dõi tất cả các địa chỉ gửi tiền của bạn trên tất cả các slot, nhưng có thể hữu ích để kiểm tra một vài tài khoản trong một khoảng thời gian cụ thể.

- Gửi một yêu cầu [`getConfirmedSignaturesForAddress2`](developing/clients/jsonrpc-api.md#getconfirmedsignaturesforaddress2) đến node api:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedSignaturesForAddress2","params":["6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC", {"limit": 3}]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby",
      "slot": 114
    },
    {
      "err": null,
      "memo": null,
      "signature": "4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr",
      "slot": 112
    },
    {
      "err": null,
      "memo": null,
      "signature": "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6",
      "slot": 108
    }
  ],
  "id": 1
}
```

- Đối với mỗi chữ ký được trả về, để nhận được chi tiết giao dịch bằng cách gửi một yêu cầu [`getConfirmedTransaction`](developing/clients/jsonrpc-api.md#getconfirmedtransaction)

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedTransaction","params":["dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6", "json"]}' localhost:8899

// Result
{
  "jsonrpc": "2.0",
  "result": {
    "slot": 5,
    "transaction": {
      "message": {
        "accountKeys": [
          "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
          "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
          "11111111111111111111111111111111"
        ],
        "header": {
          "numReadonlySignedAccounts": 0,
          "numReadonlyUnsignedAccounts": 1,
          "numRequiredSignatures": 1
        },
        "instructions": [
          {
            "accounts": [
              0,
              1
            ],
            "data": "3Bxs3zyH82bhpB8j",
            "programIdIndex": 2
          }
        ],
        "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
      },
      "signatures": [
        "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
      ]
    },
    "meta": {
      "err": null,
      "fee": 5000,
      "postBalances": [
        2033973061360,
        218099990000,
        42000000003
      ],
      "preBalances": [
        2044973066360,
        207099990000,
        42000000003
      ],
      "status": {
        "Ok": null
      }
    }
  },
  "id": 1
}
```

## Gửi lệnh Rút tiền

Để đáp ứng yêu cầu rút SOL của người dùng, bạn phải tạo một giao dịch chuyển Solana và gửi nó đến node api để được chuyển tiếp đến cụm của bạn.

### Đồng bộ

Gửi chuyển giao đồng bộ đến cụm Solana cho phép bạn dễ dàng đảm bảo rằng quá trình chuyển giao thành công và được kết thúc bởi cụm.

Công cụ dòng lệnh của Solana cung cấp một lệnh đơn giản, `solana transfer`, để tạo, gửi và xác nhận các giao dịch chuyển khoản. Theo mặc định, phương pháp này sẽ chờ đợi và theo dõi tiến trình trên stderr cho đến khi giao dịch được hoàn thành bởi cụm. Nếu giao dịch thất bại, nó sẽ thông báo lỗi của bất kì giao dịch nào.

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --keypair <KEYPAIR> --url http://localhost:8899
```

[Solana Javascript SDK](https://github.com/solana-labs/solana-web3.js) cung cấp cách tiếp cận tương tự cho hệ sinh thái JS. Sử dụng `SystemProgram` để tạo một giao dịch chuyển và gửi nó bằng phương thức này `sendAndConfirmTransaction`.

### Không đồng bộ

Để có thêm sự linh hoạt, bạn có thể gửi chuyển khoản rút tiền không đồng bộ. Trong những trường hợp này, bạn có trách nhiệm xác minh rằng giao dịch đã thành công và đã được hoàn tất bởi cụm.

**Lưu ý:** Mỗi giao dịch chứa một [blockhash gần đây](developing/programming-model/transactions.md#blockhash-format) để cho biết tính hoạt động của nó. Điều **quan trọng** là phải đợi cho đến khi blockhash này hết hạn trước khi thử lại chuyển khoản rút tiền mà dường như chưa được xác nhận hoặc hoàn tất bởi cụm. Nếu không, bạn có nguy cơ chi tiêu gấp đôi. Xem thông tin thêm về [hết hạn blockhash](#blockhash-expiration) bên dưới.

Đầu tiên, lấy một blockhash gần đây bằng cách sử dụng [`getFees` điểm cuối](developing/clients/jsonrpc-api.md#getfees) hoặc lệnh CLI:

```bash
solana fees --url http://localhost:8899
```

Trong công cụ dòng lệnh, hãy chuyển đối số `--no-wait` để gửi chuyển một cách không đồng bộ và bao gồm blockhash gần đây của bạn với đối số `--blockhash`:

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --no-wait --blockhash <RECENT_BLOCKHASH> --keypair <KEYPAIR> --url http://localhost:8899
```

Bạn cũng có thể xây dựng, ký và tuần tự hóa giao dịch theo cách thủ công và kích hoạt nó vào cụm bằng cách sử dụng JSON-RPC [`sendTransaction`điểm cuối](developing/clients/jsonrpc-api.md#sendtransaction).

#### Xác nhận Giao dịch & Tính xác thực

Nhận thông tin của một loạt giao dịch bằng cách sử dụng [`getSignatureStatuses` JSON-RPC điểm cuối](developing/clients/jsonrpc-api.md#getsignaturestatuses). Trường `confirmations` báo cáo số lượng [xác nhận các khối](../terminology.md#conf Dead-block) đã trôi qua kể từ giao dịch đã được xử lý. Nếu `confirmations: null`, thì nó [hoàn thành](../terminology.md#finality).

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getSignatureStatuses", "params":[["5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW", "5j7s6NiJS3JAkvgkoc18WVAsiSaci2pxB2A6ueCJP4tprA2TFg9wSyTLeYouxPBJEMzJinENTkpA52YStRW5Dia7"]]}' http://localhost:8899

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
        }
      },
      {
        "slot": 48,
        "confirmations": null,
        "err": null,
        "status": {
          "Ok": null
        }
      }
    ]
  },
  "id": 1
}
```

#### Hết hạn Blockhash

Khi bạn yêu cầu một blockhash gần đây cho giao dịch rút tiền của mình bằng cách sử dụng [`getFees` endpoint](developing/clients/jsonrpc-api.md#getfees) hoặc `solana fees`, phản hồi sẽ bao gồm `lastValidSlot`, slot cuối cùng trong đó blockhash sẽ có giá trị. Bạn có thể kiểm tra slot cụm bằng [`getSlot` truy vấn ](developing/clients/jsonrpc-api.md#getslot); một khi slot cụm lớn hơn `lastValidSlot`, giao dịch rút tiền bằng cách sử dụng blockhash đó sẽ không bao giờ thành công.

Bạn cũng có thể kiểm tra kỹ xem một blockhash cụ thể có còn hợp lệ hay không bằng cách gửi [`getFeeCalculatorForBlockhash`](developing/clients/jsonrpc-api.md#getfeecalculatorforblockhash) yêu cầu với blockhash làm thông số. Nếu giá trị phản hồi là null, blockhash đã hết hạn và giao dịch rút tiền sử dụng blockhash đó sẽ không bao giờ thành công.

### Xác thực địa chỉ tài khoản do người dùng cung cấp để rút tiền

Vì việc rút tiền là không thể thay đổi, nên có thể là một thực tiễn tốt để xác thực địa chỉ tài khoản do người dùng cung cấp trước khi cho phép rút tiền để ngăn chặn việc vô tình làm mất tiền của người dùng.

Địa chỉ của một tài khoản thông thường trong Solana là một chuỗi được mã hóa Base58 của một public key 256-bit ed25519. Không phải tất cả các mẫu bit đều là public key hợp lệ cho đường cong ed25519, vì vậy có thể đảm bảo địa chỉ tài khoản do người dùng cung cấp ít nhất là khóa công khai ed25519 chính xác.

#### Java

Dưới đây là một ví dụ Java về việc xác thực địa chỉ do người dùng cung cấp dưới dạng public key ed25519 hợp lệ:

Mẫu sau đây giả định rằng bạn đang sử dụng Maven.

`pom.xml`:

```xml
<repositories>
  ...
  <repository>
    <id>spring</id>
    <url>https://repo.spring.io/libs-release/</url>
  </repository>
</repositories>

...

<dependencies>
  ...
  <dependency>
      <groupId>io.github.novacrypto</groupId>
      <artifactId>Base58</artifactId>
      <version>0.1.3</version>
  </dependency>
  <dependency>
      <groupId>cafe.cryptography</groupId>
      <artifactId>curve25519-elisabeth</artifactId>
      <version>0.1.0</version>
  </dependency>
<dependencies>
```

```java
import io.github.novacrypto.base58.Base58;
import cafe.cryptography.curve25519.CompressedEdwardsY;

public class PubkeyValidator
{
    public static boolean verifyPubkey(String userProvidedPubkey)
    {
        try {
            return _verifyPubkeyInternal(userProvidedPubkey);
        } catch (Exception e) {
            return false;
        }
    }

    public static boolean _verifyPubkeyInternal(String maybePubkey) throws Exception
    {
        byte[] bytes = Base58.base58Decode(maybePubkey);
        return !(new CompressedEdwardsY(bytes)).decompress().isSmallOrder();
    }
}
```

## Hỗ trợ tiêu chuẩn Mã thông báo SPL

[Mã thông báo SPL](https://spl.solana.com/token) là tiêu chuẩn để tạo và trao đổi mã thông báo được bọc/tổng hợp trên blockchain Solana.

Quy trình làm việc của Mã thông báo SPL tương tự như quy trình của mã thông báo SOL gốc, nhưng có một số điểm khác biệt sẽ được thảo luận trong phần này.

### Mã thông báo Đúc

Mỗi _loại_ của Mã thông báo SPL được khai báo bằng cách tạo tài khoản _đúc_. Tài khoản này lưu trữ siêu dữ liệu mô tả các tính năng của mã thông báo như nguồn cung cấp, số lượng số thập phân và các cơ quan chức năng khác nhau có quyền kiểm soát đối với việc đúc tiền. Mỗi tài khoản Mã thông báo SPL tham chiếu đến cơ sở liên kết của nó và chỉ có thể tương tác với Mã thông báo SPL thuộc loại đó.

### Cài đặt Công cụ CLI `spl-token`

Tài khoản mã thông báo SPL được truy vấn và sửa đổi bằng tiện ích dòng lệnh `spl-token` Các ví dụ được cung cấp trong phần này phụ thuộc vào việc nó được cài đặt trên hệ thống cục bộ.

`spl-token` được phân phối từ Rust [crates.io](https://crates.io/crates/spl-token) thông qua `cargo` tiện ích dòng lệnh Rust. Phiên bản mới nhất của `cargo` có thể được cài đặt bằng một one-liner tiện dụng cho nền tảng của bạn tại [rustup.rs](https://rustup.rs). Sau khi cài đặt `cargo`, có thể lấy `spl-token` bằng lệnh sau:

```
cargo install spl-token-cli
```

Sau đó, bạn có thể kiểm tra phiên bản đã cài đặt để xác minh

```
spl-token --version
```

Điều này sẽ dẫn đến một cái gì đó như

```text
spl-token-cli 2.0.1
```

### Tạo Tài khoản

Tài khoản Mã thông báo SPL có các yêu cầu bổ sung mà tài khoản Chương trình hệ thống gốc không có:

1. Tài khoản Mã thông báo SPL phải được tạo ra trước khi có thể gửi một lượng mã thông báo. Tài khoản Mã thông báo có thể được tạo một cách rõ ràng bằng lệnh `spl-token create-account` hoặc hoàn toàn bằng lệnh `spl-token transfer --fund-recipient ...`.
1. Tài khoản Mã thông báo SPL phải được [miễn tiền thuê](developing/programming-model/accounts.md#rent-exemption) trong suốt thời gian tồn tại và do đó yêu cầu một lượng nhỏ mã thông báo SOL gốc được gửi khi tạo tài khoản. Đối với tài khoản Mã thông báo SPL v2, số lượng này là 0,00203928 SOL (2,039,280 lamport).

#### Dòng lệnh

Để tạo tài khoản Mã thông báo SPL với các thuộc tính sau:

1. Liên kết với đúc tiền đã cho
1. Thuộc sở hữu của keypair của tài khoản tài trợ

```
spl-token create-account <TOKEN_MINT_ADDRESS>
```

#### Ví dụ

```
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

Hoặc tạo tài khoản Mã thông báo SPL với keypair cụ thể:

```
$ solana-keygen new -o token-account.json
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir token-account.json
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

### Kiểm tra số dư tài khoản

#### Dòng lệnh

```
spl-token balance <TOKEN_ACCOUNT_ADDRESS>
```

#### Ví dụ

```
$ solana balance 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
0
```

### Chuyển Mã thông báo

Tài khoản nguồn để chuyển là tài khoản mã thông báo thực tế có chứa số tiền.

Tuy nhiên địa chỉ người nhận có thể là một tài khoản ví bình thường. Nếu tài khoản mã thông báo được liên kết cho đúc tiền đã cho không tồn tại trong ví đó, quá trình chuyển sẽ tạo nó với điều kiện là đối số `--fund-recipient` như được cung cấp.

#### Dòng lệnh

```
spl-token transfer <SENDER_ACCOUNT_ADDRESS> <AMOUNT> <RECIPIENT_WALLET_ADDRESS> --fund-recipient
```

#### Ví dụ

```
$ spl-token transfer 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN 1 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Transfer 1 tokens
  Sender: 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN
  Recipient: 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 3R6tsog17QM8KfzbcbdP4aoMfwgo6hBggJDVy7dZPVmH2xbCWjEj31JKD53NzMrf25ChFjY7Uv2dfCDq4mGFFyAj
```

### Gửi tiền

Vì mỗi cặp `(user, mint)` yêu cầu một tài khoản riêng biệt trên chuỗi, nên một sàn giao dịch nên tạo trước các lô tài khoản mã thông báo và chỉ định chúng cho người dùng theo yêu cầu. Tất cả các tài khoản này phải được sở hữu bởi các keypair do sàn giao dịch kiểm soát.

Việc giám sát các giao dịch tiền gửi phải tuân theo phương pháp [thăm dò khối](#poll-for-blocks) được mô tả ở trên. Mỗi khối mới sẽ được quét để tìm các giao dịch thành công phát hành lệnh Mã thông báo SPL [Transfer](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L92) hoặc [Transfer2](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L252) tham chiếu đến tài khoản người dùng, sau đó truy vấn và cập nhật [số dư mã thông báo của tài khoản](developing/clients/jsonrpc-api.md#gettokenaccountbalance).

Chúng tôi đang [Cân nhắc](https://github.com/solana-labs/solana/issues/12318) việc bổ sung các trường siêu dữ liệu trạng thái `preBalance` và `postBalance`giao dịch để bao gồm chuyển số dư Mã thông báo SPL.

### Rút tiền

Địa chỉ rút tiền mà người dùng cung cấp phải giống với địa chỉ được sử dụng để rút tiền SOL thông thường.

Trước khi thực hiện [chuyển khoản](#token-transfers) rút tiền, sàn giao dịch nên kiểm tra địa chỉ như [mô tả ở trên](#validating-user-supplied-account-addresses-for-withdrawals).

Từ địa chỉ rút tiền, tài khoản mã thông báo được liên kết để xác định đúng cơ sở đúc tiền và chuyển khoản được cấp cho tài khoản đó. Lưu ý rằng có thể tài khoản mã thông báo được liên kết chưa tồn tại, tại thời điểm đó, sàn giao dịch sẽ thay mặt người dùng tài trợ cho tài khoản. Đối với Mã thông báo SPL v2, việc nạp tiền vào tài khoản rút tiền sẽ yêu cầu 0,00203928 SOL (2,039,280 lamport).

Lệnh mẫu `spl-token transfer` để rút tiền:

```
$ spl-token transfer --fund-recipient <exchange token account> <withdrawal amount> <withdrawal address>
```

### Cân nhắc khác

#### Cơ quan đóng băng

Vì lý do tuân thủ quy định, pháp nhân phát hành Mã thông báo SPL có thể tùy chọn chọn giữ "Cơ quan đóng băng" trên tất cả các tài khoản được tạo liên quan đến đúc tiền của nó. Điều này cho phép họ [đóng băng](https://spl.solana.com/token#freezing-accounts) tài sản trong một tài khoản nhất định theo ý muốn, khiến tài khoản không thể sử dụng được cho đến khi tan băng. Nếu tính năng này được sử dụng, pubkey của cơ quan đóng băng sẽ được đăng ký trong tài khoản đúc tiền của Mã thông báo SPL.

## Kiểm tra Tích hợp

Đảm bảo kiểm tra toàn bộ quy trình làm việc của bạn trên [các cụm](../clusters.md) mạng dev và testnet Solana trước khi chuyển sang sản xuất trên mainnet-beta. Devnet là phần mềm mở và linh hoạt nhất, lý tưởng cho sự phát triển ban đầu, trong khi testnet cung cấp cấu hình cụm thực tế hơn. Cả devnet và testnet đều hỗ trợ một vòi, hãy chạy `solana airdrop 10` để nhận một số SOL của devnet hoặc testnet để phát triển và thử nghiệm.
