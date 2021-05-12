---
title: Ví hệ thống tệp
---

Tài liệu này mô tả cách tạo và sử dụng ví hệ thống tệp với các công cụ Solana CLI. Ví hệ thống tệp tồn tại dưới dạng tệp keypair không được mã hóa trên file hệ thống máy tính của bạn.

> Ví hệ thống tệp là phương pháp lưu trữ mã thông báo SOL **kém an toàn nhất**. **Không khuyến khích** khi lưu trữ số lượng lớn mã thông báo trên ví hệ thống tệp.

## Trước khi bắt đầu

Hãy chắc chắn rằng bạn đã [Cài đặt Công cụ dòng lệnh Solana](../cli/install-solana-cli-tools.md)

## Tạo Keypair Ví Hệ thống tệp

Sử dụng công cụ dòng lệnh của Solana `solana-keygen` để tạo tệp keypair. Ví dụ, chạy lệnh sau:

```bash
mkdir ~/my-solana-wallet
solana-keygen new --outfile ~/my-solana-wallet/my-keypair.json
```

Tệp này chứa keypair **không được mã hóa** của bạn. Trên thực tế, ngay cả khi bạn điền mật khẩu, mật khẩu đó chỉ áp dụng cho cụm từ hạt giống khôi phục, không phải cho tệp. Không chia sẻ tệp này với người khác. Bất kỳ người nào có quyền truy cập vào tệp này sẽ có quyền truy cập vào tất cả các mã thông báo được gửi đến public key của nó. Thay vào đó, bạn chỉ nên chia sẻ public key của nó. Để hiển thị public key của nó, hãy chạy:

```bash
solana-keygen pubkey ~/my-solana-wallet/my-keypair.json
```

Nó sẽ xuất ra một chuỗi các ký tự, chẳng hạn như:

```text
ErRr1caKzK8L8nn4xmEWtimYRiTCAZXjBtVphuZ5vMKy
```

Đây là public key tương ứng với keypair trong `~/my-solana-wallet/my-keypair.json`. Khóa công khai của tệp keypair là _địa chỉ ví_ của bạn.

## Xác minh Địa chỉ của bạn dựa trên tệp Keypair của bạn

Để xác minh rằng bạn giữ private key cho một địa chỉ nhất định, hãy sử dụng `solana-keygen verify`:

```bash
solana-keygen verify <PUBKEY> ~/my-solana-wallet/my-keypair.json
```

trong đó `<PUBKEY>` được thay thế bằng địa chỉ ví của bạn. Lệnh sẽ xuất ra "Success" nếu địa chỉ đã cho khớp với địa chỉ trong tệp keypair của bạn và "Failed" nếu nó không khớp.

## Tạo nhiều địa chỉ ví hệ thống tệp

Bạn có thể tạo bao nhiêu địa chỉ ví nếu thích. Chỉ cần chạy lại các bước trong [Tạo Ví Hệ Thống Tệp](#generate-a-file-system-wallet-keypair) và đảm bảo sử dụng tên tệp hoặc đường dẫn mới với `--outfile`. Nhiều địa chỉ ví có thể hữu ích nếu bạn muốn chuyển mã thông báo giữa các tài khoản của riêng bạn cho các mục đích khác nhau.
