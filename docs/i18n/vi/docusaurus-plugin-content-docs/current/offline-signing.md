---
title: Ký Giao dịch Ngoại tuyến
---

Một số mô hình bảo mật yêu cầu giữ các khóa ký và do đó, quá trình ký, tách biệt với việc tạo giao dịch và phát sóng mạng. Các ví dụ bao gồm:

- Thu thập chữ ký từ những người ký khác nhau về mặt địa lý trong [sơ đồ nhiều chữ ký](cli/usage.md#multiple-witnesses)
- Ký giao dịch bằng thiết bị ký [airgapped](https://en.wikipedia.org/wiki/Air_gap_(networking))

Tài liệu này mô tả việc sử dụng CLI của Solana để ký riêng và gửi giao dịch.

## Các Lệnh Hỗ Trợ Ký Ngoại Tuyến

Hiện tại, các lệnh sau hỗ trợ ký ngoại tuyến:

- [`create-stake-account`](cli/usage.md#solana-create-stake-account)
- [`deactivate-stake`](cli/usage.md#solana-deactivate-stake)
- [`delegate-stake`](cli/usage.md#solana-delegate-stake)
- [`split-stake`](cli/usage.md#solana-split-stake)
- [`stake-authorize`](cli/usage.md#solana-stake-authorize)
- [`stake-set-lockup`](cli/usage.md#solana-stake-set-lockup)
- [`transfer`](cli/usage.md#solana-transfer)
- [`withdraw-stake`](cli/usage.md#solana-withdraw-stake)

## Ký Giao dịch Ngoại tuyến

Để ký một giao dịch ngoại tuyến, hãy chuyển các đối số sau vào dòng lệnh

1. `--sign-only` ngăn không cho khách hàng gửi giao dịch đã ký vào mạng. Thay vào đó, các cặp pubkey/chữ ký được in thành stdout.
2. `--blockhash BASE58_HASH`, cho phép người gọi chỉ định giá trị được sử dụng để điền vào trường `recent_blockhash` của giao dịch. Điều này phục vụ một số mục đích, cụ thể là: _ Loại bỏ nhu cầu kết nối với mạng và truy vấn một blockhash gần đây thông qua qua RPC _ Cho phép người ký điều phối blockhash trong một lược đồ nhiều chữ ký

### Ví dụ: Ký Thanh toán Ngoại tuyến

Lệnh

```bash
solana@offline$ solana pay --sign-only --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    recipient-keypair.json 1
```

Đầu ra

```text

Blockhash: 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF
Signers (Pubkey=Signature):
  FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN

{"blockhash":"5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF","signers":["FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN"]}'
```

## Gửi các giao dịch đã ký ngoại tuyến lên mạng

Để gửi một giao dịch đã được ký kết ngoại tuyến tới mạng, hãy chuyển các đối số sau trên dòng lệnh

1. `--blockhash BASE58_HASH`, phải là blockhash giống như đã được sử dụng để ký
2. `--signer BASE58_PUBKEY=BASE58_SIGNATURE`, một cho mỗi người ký ngoại tuyến. Điều này bao gồm các cặp pubkey/chữ ký trực tiếp trong giao dịch thay vì ký nó bằng bất kỳ các keypair cục bộ

### Ví dụ: Gửi thanh toán đã ký ngoại tuyến

Lệnh

```bash
solana@online$ solana pay --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    --signer FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
    recipient-keypair.json 1
```

Đầu ra

```text
4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
```

## Đăng ký ngoại tuyến qua nhiều phiên

Việc ký ngoại tuyến cũng có thể diễn ra trong nhiều phiên. Trong trường hợp này, chuyển public key của người ký vắng mặt cho mỗi vai trò. Tất cả các pubkey đã được chỉ định, nhưng không có chữ ký nào được tạo ra sẽ được liệt kê là vắng mặt trong đầu ra ký ngoại tuyến

### Ví dụ: Chuyển với hai phiên ký ngoại tuyến

Lệnh (Phiên ngoại tuyến # 1)

```text
solana@offline1$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --sign-only \
    --keypair fee_payer.json \
    --from 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL
```

Đầu ra (Phiên ngoại tuyến # 1)

```text
Blockhash: 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
Signers (Pubkey=Signature):
  3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy=ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
Absent Signers (Pubkey):
  674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL
```

Lệnh (Phiên ngoại tuyến # 2)

```text
solana@offline2$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --sign-only \
    --keypair from.json \
    --fee-payer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy
```

Đầu ra (Phiên ngoại tuyến # 2)

```text
Blockhash: 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
Signers (Pubkey=Signature):
  674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL=3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ
Absent Signers (Pubkey):
  3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy
```

Lệnh (Gửi trực tuyến)

```text
solana@online$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --from 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL \
    --signer 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL=3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ \
    --fee-payer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy \
    --signer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy=ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
```

Đầu ra (Gửi trực tuyến)

```text
ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
```

## Thêm thời gian để ký

Thông thường, một giao dịch Solana phải được mạng ký kết và chấp nhận trong một số slot từ blockhash trong trường recent_blockhash của nó (~ 2 phút tại thời điểm viết bài này). Nếu thủ tục ký kết của bạn mất nhiều thời gian hơn thời gian này, [ Nonce Giao dịch Bền vững](offline-signing/durable-nonce.md) có thể cho bạn thêm thời gian cần thiết.
