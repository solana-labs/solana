---
title: Tạo public key validator
---

Để tham gia, trước tiên bạn cần đăng ký. Xem [Registration info](../registration/how-to-register.md).

Để nhận được phân bổ SOL, bạn cần xuất bản public key danh tính của validator trong tài khoản keybase.io của bạn.

## **Tạo Keypair**

1. Nếu bạn chưa có, hãy tạo keypair nhận dạng của validator bằng cách chạy:

   ```bash
     solana-keygen new -o ~/validator-keypair.json
   ```

2. Danh tính Public key có thể được xem bằng cách chạy:

   ```bash
     solana-keygen pubkey ~/validator-keypair.json
   ```

> Lưu ý: Tệp "validator-keypair.json" cũng là private key \(ed25519\) của bạn.

Keypair nhận dạng validator của bạn xác định duy nhất validator của bạn trong mạng. **Điều quan trọng là phải sao lưu thông tin này.**

Nếu bạn không sao lưu thông tin này, bạn SẼ KHÔNG CÓ KHẢ NĂNG PHỤC HỒI VALIDATOR CỦA BẠN, nếu bạn mất quyền truy cập vào nó. Nếu điều này xảy ra, BẠN SẼ MẤT SỰ CẤP QUYỀN CHO SOL.

Để sao lưu validator của bạn để nhận dạng keypair, hãy **sao lưu tệp "validator-keypair.json" của bạn vào một vị trí an toàn.**

## Liên kết pubkey Solana của bạn với tài khoản Keybase

Bạn phải liên kết pubkey Solana của mình với tài khoản Keybase.io. Hướng dẫn sau đây mô tả cách thực hiện điều đó bằng cách cài đặt Keybase trên máy chủ của bạn.

1. Cài đặt [Keybase](https://keybase.io/download) trên máy của bạn.
2. Đăng nhập vào tài khoản Keybase trên máy chủ của bạn. Trước tiên, hãy tạo tài khoản Keybase nếu bạn chưa có. Đây là [danh sách các lệnh Keybase CLI cơ bản](https://keybase.io/docs/command_line/basics).
3. Tạo thư mục Solana trong thư mục tệp công khai của bạn: `mkdir /keybase/public/<KEYBASE_USERNAME>/solana`
4. Xuất bản public key danh tính của validator của bạn bằng cách tạo một tệp trống trong thư mục tệp Keybase công khai của bạn ở định dạng sau: `/keybase/public/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>`. Ví dụ:

   ```bash
     touch /keybase/public/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>
   ```

5. Để kiểm tra public key của bạn đã được xuất bản, hãy đảm bảo bạn có thể duyệt thành công đến ` <code>https://keybase.pub/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>`
