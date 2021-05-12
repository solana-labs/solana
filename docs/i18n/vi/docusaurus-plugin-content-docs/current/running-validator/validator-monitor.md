---
title: Giám sát một validator
---

## Kiểm tra Gossip

Xác nhận địa chỉ IP và **nhận dạng pubkey** của validator của bạn được hiển thị trong mạng gossip bằng cách chạy:

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
```

## Kiểm tra số dư của bạn

Số dư tài khoản của bạn sẽ giảm theo số tiền phí giao dịch khi validator của bạn gửi phiếu bầu và tăng lên sau khi phục vụ với tư cách là leader. Vượt qua `--lamports` để quan sát chi tiết hơn:

```bash
solana balance --lamports
```

## Kiểm tra hoạt động bỏ phiếu

Các lệnh `solana vote-account` hiển thị các hoạt động bầu cử gần đây từ validator của bạn:

```bash
solana vote-account ~/vote-account-keypair.json
```

## Nhận thông tin cụm

Có một số điểm cuối JSON-RPC hữu ích để theo dõi validator của bạn trên cụm, cũng như tình trạng của cụm:

```bash
# Similar to solana-gossip, you should see your validator in the list of cluster nodes
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getClusterNodes"}' http://devnet.solana.com
# If your validator is properly voting, it should appear in the list of `current` vote accounts. If staked, `stake` should be > 0
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}' http://devnet.solana.com
# Returns the current leader schedule
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}' http://devnet.solana.com
# Returns info about the current epoch. slotIndex sẽ tiến triển trong các cuộc gọi tiếp theo.
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://devnet.solana.com
```
