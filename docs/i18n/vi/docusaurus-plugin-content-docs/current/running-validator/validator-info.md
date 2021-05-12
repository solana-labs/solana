---
title: Xuất bản thông tin validator
---

Bạn có thể xuất bản thông tin validator của mình lên chuỗi để hiển thị công khai cho những người dùng khác.

## Chạy thông tin validator solana

Chạy solana CLI để điền thông tin tài khoản validator:

```bash
solana validator-info publish --keypair ~/validator-keypair.json <VALIDATOR_INFO_ARGS> <VALIDATOR_NAME>
```

Để biết chi tiết về các trường tùy chọn cho VALIDATOR_INFO_ARGS:

```bash
solana validator-info publish --help
```

## Lệnh ví dụ

Lệnh xuất bản ví dụ:

```bash
solana validator-info publish "Elvis Validator" -n elvis -w "https://elvis-validates.com"
```

Lệnh truy vấn ví dụ:

```bash
solana validator-info get
```

đầu ra

```text
Validator info from 8WdJvDz6obhADdxpGCiJKZsDYwTLNEDFizayqziDc9ah
  Validator pubkey: 6dMH3u76qZ7XG4bVboVRnBHR2FfrxEqTTTyj4xmyDMWo
  Info: {"keybaseUsername":"elvis","name":"Elvis Validator","website":"https://elvis-validates.com"}
```

## Keybase

Bao gồm tên người dùng Keybase cho phép các ứng dụng khách hàng \(như Trình khám phá mạng Solana\) để tự động lấy hồ sơ công khai validator của bạn, bao gồm bằng chứng mật mã, nhận dạng thương hiệu, v. v. Để kết nối pubkey validator của bạn với Keybase:

1. Tham gia [https://keybase.io/](https://keybase.io/) và hoàn thành hồ sơ cho validator của bạn
2. Thêm **identity pubkey** validator của bạn vào Keybase:

   - Tạo một têp trống trên máy tính cục bộ của bạn có tên `validator-<PUBKEY>`
   - Trong Keybase, điều hướng đến phần Tệp và tải tệp pubkey của bạn lên

     thư mục con `solana` trong thư mục công khai của bạn: `/keybase/public/<KEYBASE_USERNAME>/solana`

   - Để kiểm tra mã pubkey của bạn, hãy đảm bảo bạn có thể duyệt thành công

     `https://keybase.pub/<KEYBASE_USERNAME>/solana/validator-<PUBKEY>`

3. Thêm hoặc cập nhật `solana validator-info` với tên người dùng Keybase của bạn. Các

   CLI sẽ xác minh tệp `validator-<PUBKEY>`
