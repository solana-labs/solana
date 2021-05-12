## Khởi động lại một cụm

### Bước 1. Xác định slot mà cụm sẽ được khởi động lại tại

Slot được xác nhận lạc quan cao nhất là slot tốt nhất để bắt đầu, có thể được tìm thấy bằng cách tìm kiếm điểm dữ liệu số liệu [này](https://github.com/solana-labs/solana/blob/0264147d42d506fb888f5c4c021a998e231a3e74/core/src/optimistic_confirmation_verifier.rs#L71). Nếu không, hãy sử dụng gốc cuối cùng.

Gọi slot này `SLOT_X`

### Bước 2. Dừng (các) validator

### Bước 3. Tùy chọn cài đặt phiên bản solana mới

### Bước 4. Tạo một ảnh chụp nhanh mới cho slot `SLOT_X` bằng một hard fork tại slot `SLOT_X`

```bash
$ solana-ledger-tool -l ledger create-snapshot SLOT_X ledger --hard-fork SLOT_X
```

Thư mục sổ cái bây giờ sẽ chứa ảnh chụp nhanh mới. `solana-ledger-tool create-snapshot` cũng sẽ xuất ra phiên bản shred mới và giá trị hàm băm ngân hàng, gọi là NEW_SHRED_VERSION and NEW_BANK_HASH.

Điều chỉnh các đối số validator của bạn:

```bash
 --wait-for-supermajority SLOT_X
 --expected-bank-hash NEW_BANK_HASH
```

Sau đó khởi động lại validator.

Xác nhận với nhật ký rằng validator đã khởi động và hiện đang ở trạng thái giữ tại `SLOT_X`, chờ đợi một siêu đa số.

### Bước 5. Thông báo khởi động lại trên Discord:

Đăng một cái gì đó như sau lên #announments (điều chỉnh văn bản cho phù hợp):

> Xin chào @Validators,
>
> Chúng tôi đã phát hành v1.1.12 và sẵn sàng khôi phục lại testnet.
>
> Các bước: 1. Cài đặt bản phát hành v1.1.12: https://github.com/solana-labs/solana/releases/tag/v1.1.12 2. a. Phương pháp ưa thích, bắt đầu từ sổ cái cục bộ của bạn với:
>
> ```bash
> solana-validator
>   --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --hard-fork SLOT_X                  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --no-snapshot-fetch                 # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --entrypoint entrypoint.testnet.solana.com:8001
>   --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
>   --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
>   --no-untrusted-rpc
>   --limit-ledger-size
>   ...                                # <-- your other --identity/--vote-account/etc arguments
> ```

````

b. Nếu validator của bạn không có sổ cái cho đến thời điểm SLOT_X hoặc nếu bạn đã xóa sổ cái của mình, thay vào đó, hãy tải xuống một ảnh chụp nhanh với:

```bash
solana-validator
  --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --entrypoint entrypoint.testnet.solana.com:8001
  --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
  --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
  --no-untrusted-rpc
  --limit-ledger-size
  ...                                # <-- your other --identity/--vote-account/etc arguments
````

     You can check for which slots your ledger has with: `solana-ledger-tool -l path/to/ledger bounds`

3. Chờ cho đến khi 80% của stake được trực tuyến

Để xác nhận rằng validator được khởi động lại của bạn đã chính xác, hãy đợi 80%: a. Tìm kiếm ghi nhật ký tin nhắn `N% of active stake visible in gossip`. Hỏi nó qua RPC slot nào trên đó: `solana --url http://127.0.0.1:8899 slot`. Nó sẽ trả về `SLOT_X` cho đến khi chúng tôi nhận được 80% stake

Cảm ơn!

### Bước 7. Chờ và lắng nghe

Giám sát validator khi chúng khởi động lại. Trả lời các câu hỏi, trợ giúp mọi người,
