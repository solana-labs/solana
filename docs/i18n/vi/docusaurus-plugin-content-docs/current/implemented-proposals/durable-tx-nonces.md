---
title: Giao dịch bền
---

## Sự cố

Để ngăn phát lại, các giao dịch Solana chứa trường nonce được điền với giá trị blockhash "gần đây". Một giao dịch có chứa blockhash quá cũ (~ 2 phút tính đến thời điểm viết bài này) bị mạng từ chối là không hợp lệ. Thật không may, một số trường hợp sử dụng, chẳng hạn như dịch vụ trông coi, đòi hỏi nhiều thời gian hơn để tạo chữ ký cho giao dịch. Cần có cơ chế để cho phép những người tham gia mạng ngoại tuyến tiềm năng này.

## Yêu cầu

1. Chữ ký của giao dịch cần phải bao gồm giá trị nonce
2. Nonce không được sử dụng lại, ngay cả trong trường hợp ký tiết lộ khóa

## Giải pháp dựa trên hợp đồng

Ở đây chúng tôi mô tả một giải pháp dựa trên hợp đồng cho vấn đề, theo đó khách hàng có thể "lưu trữ" một giá trị nonce để sử dụng trong tương lai trong `recent_blockhash` của giao dịch. Cách tiếp cận này giống với hướng dẫn nguyên tử So sánh và Hoán đổi, được thực hiện bởi một số ISA CPU.

Khi sử dụng nonce lâu dài, trước tiên khách hàng phải tìm kiếm giá trị của nó từ dữ liệu tài khoản. Một giao dịch hiện được xây dựng theo cách bình thường, nhưng cần các yêu cầu bổ sung sau:

1. Giá trị nonce lâu dài được sử dụng trong `recent_blockhash`
2. `AdvanceNonceAccount` Một hướng dẫn được phát hành đầu tiên trong giao dịch

### Cơ chế Hợp đồng

VIỆC CẦN LÀM: svgbob điều này vào một lưu đồ

```text
Start
Create Account
  state = Uninitialized
NonceInstruction
  if state == Uninitialized
    if account.balance < rent_exempt
      error InsufficientFunds
    state = Initialized
  elif state != Initialized
    error BadState
  if sysvar.recent_blockhashes.is_empty()
    error EmptyRecentBlockhashes
  if !sysvar.recent_blockhashes.contains(stored_nonce)
    error NotReady
  stored_hash = sysvar.recent_blockhashes[0]
  success
WithdrawInstruction(to, lamports)
  if state == Uninitialized
    if !signers.contains(owner)
      error MissingRequiredSignatures
  elif state == Initialized
    if !sysvar.recent_blockhashes.contains(stored_nonce)
      error NotReady
    if lamports != account.balance && lamports + rent_exempt > account.balance
      error InsufficientFunds
  account.balance -= lamports
  to.balance += lamports
  success
```

Khách hàng muốn sử dụng tính năng này bắt đầu bằng cách tạo một tài khoản nonce trong chương trình hệ thống. Tài khoản này sẽ ở trạng thái `Uninitialized` không có hàm băm được lưu trữ, và do đó không sử dụng được.

Để khởi tạo một tài khoản mới, một hướng dẫn `InitializeNonceAccount` phải được đưa ra. Hướng dẫn này có một thông số, `Pubkey`của tài khoản [thẩm quyền](../offline-signing/durable-nonce.md#nonce-authority). Các tài khoản Nonce phải được [miễn tiền thuê](rent.md#two-tiered-rent-regime) để đáp ứng các yêu cầu về độ bền dữ liệu của tính năng, và do đó, yêu cầu phải nạp đủ số lượng lamport trước khi có thể khởi chạy chúng. Sau khi khởi tạo thành công, blockhash gần đây nhất của cụm được lưu trữ cùng với thẩm quyền nonce được chỉ định `Pubkey`.

`AdvanceNonceAccount` được sử dụng để quản lý giá trị nonce được lưu trữ của tài khoản. Nó lưu trữ blockhash gần đây nhất của cụm trong dữ liệu trạng thái của tài khoản, không thành công nếu điều đó khớp với giá trị đã được lưu trữ ở đó. Việc kiểm tra này ngăn việc phát lại các giao dịch trong cùng một khối.

Do yêu cầu [miễn tiền thuê](rent.md#two-tiered-rent-regime) của tài khoản nonce, một hướng dẫn rút tiền tùy chỉnh được sử dụng để chuyển tiền ra khỏi tài khoản. `WithdrawNonceAccount` có một cuộc tranh luận, các lamport rút, và thực thi miễn tiền thuê bằng cách ngăn số dư tài khoản giảm xuống dưới mức tối thiểu được miễn tiền thuê. Một ngoại lệ đối với séc này là nếu số dư cuối cùng sẽ là không lamport, điều này sẽ làm cho tài khoản đủ điều kiện để xóa. Chi tiết đóng tài khoản này có một yêu cầu bổ sung là giá trị nonce được lưu trữ không được khớp với blockhash gần đây nhất của cụm, theo `AdvanceNonceAccount`.

Tài khoản [không có thẩm quyền](../offline-signing/durable-nonce.md#nonce-authority) có thể được thay đổi bằng cách sử dụng hướng dẫn `AuthorizeNonceAccount`. Nó nhận một thông số, `Pubkey` của cơ quan mới. Việc thực hiện hướng dẫn này sẽ cấp toàn quyền kiểm soát tài khoản và số dư của tài khoản cho người có thẩm quyền mới.

> `AdvanceNonceAccount`, <`WithdrawNonceAccount` và `AuthorizeNonceAccount` tất cả đều yêu cầu [ không có thẩm quyền](../offline-signing/durable-nonce.md#nonce-authority) cho tài khoản để ký giao dịch.

### Hỗ trợ Thời gian chạy

Chỉ riêng hợp đồng là không đủ để triển khai tính năng này. Ép buộc một `recent_blockhash` còn tồn tại trong giao dịch và ngăn chặn hành vi trộm cắp phí thông qua việc phát lại giao dịch không thành công, cần phải sửa đổi thời gian chạy.

Bất kỳ giao dịch nào không được `check_hash_age` xác thực thông thường sẽ được kiểm tra để xác định Giao dịch Nonce. Điều này được báo hiệu bằng cách bao gồm lệnh `AdvanceNonceAccount` làm lệnh đầu tiên trong giao dịch.

Nếu thời gian chạy xác định rằng Durable Transaction Nonce đang được sử dụng, nó sẽ thực hiện các hành động bổ sung sau để xác thực giao dịch:

1. `NonceAccount` được chỉ định trong hướng dẫn `Nonce` được tải.
2. `NonceState` được giải mã từ trường dữ liệu của `NonceAccount` và được xác nhận là ở trạng thái `Initialized`.
3. Giá trị nonce được lưu trữ trong `NonceAccount` được kiểm tra để khớp với giá trị được chỉ định trong trường của giao dịch `recent_blockhash`.

Nếu cả ba lần kiểm tra trên đều thành công, giao dịch được phép tiếp tục xác thực.

Vì các giao dịch không thành công với `InstructionError` sẽ bị tính phí và các thay đổi đối với trạng thái của chúng sẽ được khôi phục lại, nên có cơ hội xảy ra hành vi trộm cắp phí nếu lệnh `AdvanceNonceAccount` được hoàn nguyên. Validator độc hại có thể phát lại giao dịch không thành công cho đến khi nonce được lưu trữ được nâng cao thành công. Thay đổi thời gian chạy ngăn chặn hành vi này. Khi một giao dịch nonce lâu dài không thành công với `InstructionError` ngoài hướng dẫn `AdvanceNonceAccount`, tài khoản nonce sẽ được quay trở lại trạng thái trước khi thực hiện như bình thường. Sau đó, thời gian chạy tăng giá trị nonce của nó và tài khoản nonce nâng cao được lưu trữ như thể nó đã thành công.
