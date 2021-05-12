---
title: Quản lý Tài Khoản Stake
---

Nếu bạn muốn ủy quyền stake cho nhiều validator khác nhau, bạn sẽ cần tạo một tài khoản để stake riêng cho từng người. Nếu bạn tuân theo quy ước tạo tài khoản stake đầu tiên ở hạt giống "0", tài khoản thứ hai ở "1", thứ ba ở "2", v. v., thì công cụ `solana-stake-accounts` sẽ cho phép bạn hoạt động trên tất cả tài khoản chỉ với một lần gọi. Bạn có thể sử dụng nó để tổng hợp số dư của tất cả các tài khoản, chuyển tài khoản sang ví mới hoặc thiết lập quyền hạn mới.

## Sử dụng

### Tạo tài khoản stake

Tạo và cấp vốn cho một tài khoản stake tại public key của stake authority:

```bash
solana-stake-accounts new <FUNDING_KEYPAIR> <BASE_KEYPAIR> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

### Đếm tài khoản

Đếm số lượng tài khoản dẫn xuất:

```bash
solana-stake-accounts count <BASE_PUBKEY>
```

### Nhận số dư tài khoản stake

Tổng số dư của các tài khoản stake có nguồn gốc:

```bash
solana-stake-accounts balance <BASE_PUBKEY> --num-accounts <NUMBER>
```

### Nhận địa chỉ tài khoản stake

Liệt kê từng địa chỉ của mỗi tài khoản stake có được từ public key đã cho:

```bash
solana-stake-accounts addresses <BASE_PUBKEY> --num-accounts <NUMBER>
```

### Đặt quyền hạn mới

Đặt quyền hạn mới trên mỗi tài khoản stake:

```bash
solana-stake-accounts authorize <BASE_PUBKEY> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdraw-authority <PUBKEY> \
    --num-accounts <NUMBER> --fee-payer <KEYPAIR>
```

### Chuyển các tài khoản stake

Chuyển các tài khoản stake:

```bash
solana-stake-accounts rebase <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --num-accounts <NUMBER> \
    --fee-payer <KEYPAIR>
```

Để cơ sở lại nguyên bản và ủy quyền cho từng tài khoản stake, hãy sử dụng lệnh 'move' chỉ huy:

```bash
solana-stake-accounts move <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdraw-authority <PUBKEY> \
    --num-accounts <NUMBER> --fee-payer <KEYPAIR>
```
