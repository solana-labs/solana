---
title: Staking
---

**Theo mặc định, validator của bạn không có stake.** Điều này có nghĩa là sẽ không đủ điều kiện để trở thành leader.

## Bắt kịp Giám sát

Để ủy quyền stake, trước tiên hãy đảm bảo validator của bạn đang chạy và bắt kịp cụm. Có thể mất một thời gian để bắt kịp sau validator của bạn khởi động. Sử dụng lệnh `catchup` để giám sát validator của bạn trong quá trình này:

```bash
solana catchup ~/validator-keypair.json
```

Cho đến khi validator của bạn bắt kịp, nó sẽ không thể bỏ phiếu thành công và không thể ủy quyền stake cho nó.

Ngoài ra, nếu bạn thấy slot của cụm tiến triển nhanh hơn của bạn, bạn có thể sẽ không bao giờ bắt kịp. Điều này thường ngụ ý một số loại vấn đề mạng giữa validator của bạn và phần còn lại của cụm.

## Tạo Keypair Stake

Nếu bạn chưa làm như vậy, hãy tạo keypair để staking. Nếu bạn đã hoàn thành bước này, bạn sẽ thấy “validator-stake-keypair.json” trong thư mục thời gian chạy Solana của mình.

```bash
solana-keygen new -o ~/validator-stake-keypair.json
```

## Ủy quyền Stake

Bây giờ ủy quyền 1 SOL cho validator của bạn bằng cách tạo tài khoản stake của bạn trước:

```bash
solana create-stake-account ~/validator-stake-keypair.json 1
```

và sau đó ủy quyền stake đó cho validator của bạn:

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/vote-account-keypair.json
```

> Không ủy quyền SOL còn lại của bạn, vì validator của bạn sẽ sử dụng các mã thông báo đó để bỏ phiếu.

Stake có thể được ủy quyền lại cho một node khác bất kỳ lúc nào bằng cùng một lệnh, nhưng chỉ một lần được ủy quyền lại trong mỗi kỷ nguyên:

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/some-other-vote-account-keypair.json
```

Giả sử node đang bỏ phiếu, bây giờ bạn đang thiết lập chạy và tạo phần thưởng validator. Phần thưởng được trả tự động theo ranh giới kỷ nguyên.

Phần thưởng mà các lamport nhận được được chia giữa tài khoản stake của bạn và tài khoản bỏ phiếu theo tỷ lệ hoa hồng được đặt trong tài khoản bỏ phiếu. Bạn chỉ có thể kiếm được phần thưởng khi validator đang hoạt động. Hơn nữa, khi đã stake, validator sẽ trở thành một phần quan trọng của mạng. Để xóa validator khỏi mạng một cách an toàn, trước tiên hãy deactivate stake của họ.

Vào cuối mỗi slot, validator sẽ gửi một giao dịch biểu quyết. Các giao dịch bỏ phiếu này được thanh toán bằng các lamport từ tài khoản của validator.

Đây là một giao dịch thông thường nên phí giao dịch tiêu chuẩn sẽ được áp dụng. Phạm vi phí giao dịch được xác định bởi khối gốc. Phí thực tế sẽ dao động dựa trên lượng giao dịch. Bạn có thể xác định mức phí hiện tại thông qua [RPC API “getRecentBlockhash”](developing/clients/jsonrpc-api.md#getrecentblockhash) trước khi gửi một giao dịch.

Tìm hiểu thêm về [phí giao dịch tại đây](../implemented-proposals/transaction-fees.md).

## Khởi động Validator Stake

Để chống lại các cuộc tấn công khác nhau vào sự đồng thuận, các ủy quyền stake mới phải tuân theo giai đoạn [khởi động](/staking/stake-accounts#delegation-warmup-and-cooldown).

Theo dõi stake của validator trong giai đoạn khởi động bằng cách:

- Xem tài khoản phiếu bầu của bạn:`solana vote-account ~/vote-account-keypair.json` Hiển thị trạng thái hiện tại của tất cả các phiếu bầu mà validator đã gửi cho mạng.
- `solana stake-account ~/validator-stake-keypair.json` Xem tài khoản stake của bạn, tùy chọn ủy quyền và chi tiết về stake
- `solana validators` hiển thị số stake đang hoạt động hiện tại của tất cả các validator, bao gồm cả của bạn
- `solana stake-history` hiển thị lịch sử cổ phần nóng lên và hạ nhiệt trong các kỷ nguyên gần đây
- Thông báo nhật ký trên validator của bạn cho biết slot dẫn đầu tiếp theo của bạn: `[2019-09-27T20:16:00.319721164Z INFO solana_core::replay_stage] <VALIDATOR_IDENTITY_PUBKEY> voted and reset PoH at tick height ####. My next leader slot is ####`
- Khi stake của bạn được làm nóng, bạn sẽ thấy số dư stake được liệt kê cho validator của bạn bằng cách chạy `solana validators`

## Theo Dõi Validator Bạn Đã Uỷ Thác Stake

Xác nhận validator của bạn trở thành [leader](../terminology.md#leader)

- Sau khi validator của bạn được bắt kịp, hãy sử dụng lệnh `solana balance` để theo dõi thu nhập khi validator của bạn được chọn làm leader và thu phí giao dịch
- Các node Solana cung cấp một số phương pháp JSON-RPC hữu ích để trả về thông tin về mạng và sự tham gia của validator của bạn. Đưa ra yêu cầu bằng cách sử dụng curl \(hoặc một khách hàng http khác mà bạn chọn\), chỉ định phương thức mong muốn trong dữ liệu có định dạng JSON-RPC. Ví dụ:

```bash
  // Request
  curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://localhost:8899

  // Result
  {"jsonrpc":"2.0","result":{"epoch":3,"slotIndex":126,"slotsInEpoch":256},"id":1}
```

Helpful JSON-RPC methods:

- `getEpochInfo`[Epoch](../terminology.md#epoch)là thời gian, tức là số lượng [các slot](../terminology.md#slot),[lịch trình của leader](../terminology.md#leader-schedule) có hiệu lực. Điều này sẽ cho bạn biết kỷ nguyên hiện tại là gì và khoảng cách của cụm.
- `getVoteAccounts` Điều này sẽ cho bạn biết validator của bạn hiện có bao nhiêu stake đang hoạt động. % stake của validator được kích hoạt trên ranh giới kỷ nguyên. Bạn có thể tìm hiểu thêm về cách staking trên Solana [tại đây](../cluster/stake-delegation-and-rewards.md).
- `getLeaderSchedule` Tại bất kỳ thời điểm nào, mạng luôn mong đợi có validator để tạo các mục nhập sổ cái. [Các validator hiện đang được chọn để tạo các mục nhập sổ cái](../cluster/leader-rotation.md#leader-rotation) được gọi là “leader”. Thao tác này sẽ trả về lịch trình hoàn chỉnh của leader \(trên cơ sở từng slot\) cho số stake hiện đang được kích hoạt, pubkey nhận dạng sẽ hiển thị 1 hoặc nhiều lần tại đây.

## Hủy Kích Hoạt Stake

Trước khi tách validator của bạn khỏi cụm, bạn nên hủy kích hoạt stake đã ủy quyền trước đó bằng cách chạy:

```bash
solana deactivate-stake ~/validator-stake-keypair.json
```

Stake không bị vô hiệu hóa ngay lập tức và thay vào đó sẽ nguội đi theo cách tương tự như lúc stake được nóng lên. Validator của bạn sẽ vẫn được gắn vào cụm trong khi stake đang hạ nhiệt. Trong khi hạ nhiệt, stake của bạn sẽ tiếp tục nhận được phần thưởng. Chỉ sau khi stake hạ nhiệt thì mới có thể an toàn để tắt validator của bạn hoặc rút nó khỏi mạng. Thời gian hạ nhiệt có thể mất vài kỷ nguyên để hoàn thành, tùy thuộc vào số tiền stake đang hoạt động và quy mô stake của bạn.

Lưu ý rằng tài khoản stake chỉ có thể được sử dụng một lần, vì vậy sau khi hủy kích hoạt, hãy sử dụng lệnh `withdraw-stake` của cli để khôi phục các lamport đã stake trước đó.
