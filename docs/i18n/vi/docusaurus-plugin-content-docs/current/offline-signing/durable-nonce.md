---
title: Giao dịch bền Nonce
---

Các giao dịch lâu dài Nonce là một cơ chế để vượt qua vòng đời ngắn điển hình của [`recent_blockhash`](developing/programming-model/transactions.md#recent-blockhash). Chúng được triển khai dưới dạng Chương trình Solana, cơ chế của chương trình này có thể được đọc trong [đề nghị](../implemented-proposals/durable-tx-nonces.md).

## Ví dụ sử dụng

Chi tiết sử dụng đầy đủ cho các lệnh CLI nonce lâu dài có thể được tìm thấy trong [Tham chiếu CLI](../cli/usage.md).

### Cơ quan Nonce

Quyền hạn đối với tài khoản nonce có thể được tùy chọn gán cho tài khoản khác. Khi làm như vậy, cơ quan mới kế thừa toàn quyền kiểm soát tài khoản nonce từ cơ quan trước đó, bao gồm cả người tạo tài khoản. Tính năng này cho phép tạo ra các thỏa thuận quyền sở hữu tài khoản phức tạp hơn và các địa chỉ tài khoản có nguồn gốc không được liên kết với một keypair. Đối số `--nonce-authority <AUTHORITY_KEYPAIR>` được sử dụng để chỉ định tài khoản này và được hỗ trợ bởi các lệnh sau

- `create-nonce-account`
- `new-nonce`
- `withdraw-from-nonce-account`
- `authorize-nonce-account`

### Tạo tài khoản nonce

Tính năng nonce giao dịch lâu dài sử dụng một tài khoản để lưu trữ giá trị nonce tiếp theo. Tài khoản nonce lâu dài phải được [miễn-tiền thuê](../implemented-proposals/rent.md#two-tiered-rent-regime), vì vậy cần phải thực hiện số dư tối thiểu để đạt được điều này.

Tài khoản nonce được tạo trước tiên bằng cách tạo keypair mới, sau đó tạo tài khoản trên chuỗi

- Lệnh

```bash
solana-keygen new -o nonce-keypair.json
solana create-nonce-account nonce-keypair.json 1
```

- Đầu ra

```text
2SymGjGV4ksPdpbaqWFiDoBz8okvtiik4KE9cnMQgRHrRLySSdZ6jrEcpPifW4xUpp4z66XM9d9wM48sA7peG2XL
```

> Để giữ cho keypair hoàn toàn ngoại tuyến, hãy sử dụng[các hướng dẫn](wallet-guide/paper-wallet.md#seed-phrase-generation) tạo keypair [Ví Giấy](wallet-guide/paper-wallet.md) thay thế

> [Tài liệu sử dụng đầy đủ](../cli/usage.md#solana-create-nonce-account)

### Truy vấn giá trị Nonce được lưu trữ

Tạo một giao dịch nonce lâu dài yêu cầu chuyển giá trị nonce được lưu trữ làm giá trị cho đối số `--blockhash` khi ký và gửi. Nhận giá trị nonce được lưu trữ hiện tại với

- Lệnh

```bash
solana nonce nonce-keypair.json
```

- Đầu ra

```text
8GRipryfxcsxN8mAGjy8zbFo9ezaUsh47TsPzmZbuytU
```

> [Tài liệu sử dụng đầy đủ](../cli/usage.md#solana-get-nonce)

### Nâng cao giá trị Nonce được lưu trữ

Mặc dù thường không cần thiết bên ngoài một giao dịch hữu ích hơn, nhưng nonce được lưu trữ giá trị có thể được nâng cao bởi

- Lệnh

```bash
solana new-nonce nonce-keypair.json
```

- Đầu ra

```text
44jYe1yPKrjuYDmoFTdgPjg8LFpYyh1PFKJqm5SC1PiSyAL8iw1bhadcAX1SL7KDmREEkmHpYvreKoNv6fZgfvUK
```

> [Tài liệu sử dụng đầy đủ](../cli/usage.md#solana-new-nonce)

### Hiển thị Tài khoản Nonce

Kiểm tra tài khoản nonce ở định dạng thân thiện với con người hơn với

- Lệnh

```bash
solana nonce-account nonce-keypair.json
```

- Đầu ra

```text
balance: 0.5 SOL
minimum balance required: 0.00136416 SOL
nonce: DZar6t2EaCFQTbUP4DHKwZ1wT8gCPW2aRfkVWhydkBvS
```

> [Tài liệu sử dụng đầy đủ](../cli/usage.md#solana-nonce-account)

### Rút tiền từ Tài khoản Nonce

Rút tiền từ tài khoản nonce với

- Lệnh

```bash
solana withdraw-from-nonce-account nonce-keypair.json ~/.config/solana/id.json 0.5
```

- Đầu ra

```text
3foNy1SBqwXSsfSfTdmYKDuhnVheRnKXpoPySiUDBVeDEs6iMVokgqm7AqfTjbk7QBE8mqomvMUMNQhtdMvFLide
```

> Đóng tài khoản nonce bằng cách rút toàn bộ số dư

> [Tài liệu sử dụng đầy đủ](../cli/usage.md#solana-withdraw-from-nonce-account)

### Chỉ định một Thẩm quyền mới cho một Tài khoản Nonce

Chỉ định lại quyền hạn của tài khoản nonce sau khi được tạo với

- Lệnh

```bash
solana authorize-nonce-account nonce-keypair.json nonce-authority.json
```

- Đầu ra

```text
3F9cg4zN9wHxLGx4c3cUKmqpej4oa67QbALmChsJbfxTgTffRiL3iUehVhR9wQmWgPua66jPuAYeL1K2pYYjbNoT
```

> [Tài liệu sử dụng đầy đủ](../cli/usage.md#solana-authorize-nonce-account)

## Các lệnh khác hỗ trợ Nonce lâu dài

Để sử dụng các lệnh nonce lâu dài với các lệnh con CLI khác, hai đối số phải được hỗ trợ.

- `--nonce`, chỉ định tài khoản lưu trữ giá trị nonce
- `--nonce-authority`, chỉ định một [quyền hạn nonce](#nonce-authority) tùy chọn

Các lệnh con sau đã nhận được cách xử lý này cho đến nay

- [`pay`](../cli/usage.md#solana-pay)
- [`delegate-stake`](../cli/usage.md#solana-delegate-stake)
- [`deactivate-stake`](../cli/usage.md#solana-deactivate-stake)

### Ví dụ thanh toán bằng Nonce lâu dài

Ở đây chúng tôi chứng minh Alice thanh toán cho Bob 1 SOL bằng cách sử dụng nonce lâu dài. Quy trình này giống nhau đối với tất cả các lệnh con hỗ trợ các nonces lâu dài

#### - Tạo tài khoản

Đầu tiên, chúng tôi cần các tài khoản cho Alice, nonce Alice và Bob

```bash
$ solana-keygen new -o alice.json
$ solana-keygen new -o nonce.json
$ solana-keygen new -o bob.json
```

#### - Cấp vốn vào tài khoản của Alice

Alice sẽ cần một số tiền để tạo một tài khoản nonce và gửi cho Bob. Airdrop cho cô ấy một số SOL

```bash
$ solana airdrop -k alice.json 10
10 SOL
```

#### - Tạo tài khoản nonce cho Alice

Bây giờ, Alice cần một tài khoản nonce. Tạo một cái

> Tại đây, không có [thẩm quyền nonce](#nonce-authority) riêng biệt nào được sử dụng, vì vậy `alice.json` có toàn quyền đối với tài khoản nonce

```bash
$ solana create-nonce-account -k alice.json nonce.json 1
3KPZr96BTsL3hqera9up82KAU462Gz31xjqJ6eHUAjF935Yf8i1kmfEbo6SVbNaACKE5z6gySrNjVRvmS8DcPuwV
```

#### - Nỗ lực đầu tiên không thành công để thanh toán cho Bob

Alice cố gắng thanh toán cho Bob, nhưng mất quá nhiều thời gian để ký. Blockhash được chỉ định hết hạn và giao dịch không thành công

```bash
$ solana pay -k alice.json --blockhash expiredDTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 bob.json 1
[2020-01-02T18:48:28.462911000Z ERROR solana_cli::cli] Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
Error: Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
```

#### - Nonce để giải cứu!

Alice thử lại giao dịch, lần này chỉ định tài khoản nonce của cô ấy và blockhash được lưu trữ ở đó

> Hãy nhớ rằng, `alice.json` là [thẩm quyền nonce](#nonce-authority) trong ví dụ này

```bash
$ solana nonce-account nonce.json
balance: 1 SOL
minimum balance required: 0.00136416 SOL
nonce: F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7
```

```bash
$ solana pay -k alice.json --blockhash F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 --nonce nonce.json bob.json 1
HR1368UKHVZyenmH7yVz5sBAijV6XAPeWbEiXEGVYQorRMcoijeNAbzZqEZiH8cDB8tk65ckqeegFjK8dHwNFgQ
```

#### - Thành công!

Giao dịch thành công! Bob nhận được 1 SOL từ Alice và nonce được lưu trữ của Alice là một giá trị mới

```bash
$ solana balance -k bob.json
1 SOL
```

```bash
$ solana nonce-account nonce.json
balance: 1 SOL
minimum balance required: 0.00136416 SOL
nonce: 6bjroqDcZgTv6Vavhqf81oBHTv3aMnX19UTB51YhAZnN
```
