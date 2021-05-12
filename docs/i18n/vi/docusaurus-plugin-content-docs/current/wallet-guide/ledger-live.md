---
title: Ledger Nano S và Nano X
---

Tài liệu này mô tả cách thiết lập [Ledger Nano S](https://shop.ledger.com/products/ledger-nano-s) hoặc [Ledger Nano X](https://shop.ledger.com/pages/ledger-nano-x) bằng phần mềm [Ledger Live](https://www.ledger.com/ledger-live).

Sau khi các bước thiết lập hiển thị bên dưới hoàn tất và ứng dụng Solana đã được cài đặt trên thiết bị Nano của bạn, người dùng có một số tùy chọn về [cách sử dụng Nano để tương tác với Mạng Solana](#interact-with-the-solana-network)

## Bắt đầu

- Đặt hàng [Nano S](https://shop.ledger.com/products/ledger-nano-s) và [Nano X](https://shop.ledger.com/pages/ledger-nano-x) từ Ledger.
- Làm theo các hướng dẫn để thiết lập thiết bị, hoặc [trang bắt đầu của Ledger](https://www.ledger.com/start/)
- Cài đặt [phần mềm Ledger Live cho máy tính](https://www.ledger.com/ledger-live/)
  - Nếu bạn đã cài đặt Ledger Live, vui lòng cập nhật phiên bản Ledger Live mới nhất, cho phép cập nhật chương trình cơ sở và các ứng dụng mới nhất.
- Kết nối Nano với máy tính của bạn và làm theo hướng dẫn trên màn hình.
- Cập nhật chương trình cơ sở trên Nano mới của bạn.  Điều này là cần thiết để đảm bảo bạn có thể cài đặt các phiên bản mới nhất của Ứng dụng Solana.
  - [Cập nhật chương trình cơ sở Nano S](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [Cập nhật chương trình cơ sở Nano X](https://support.ledger.com/hc/en-us/articles/360013349800)

## Cài đặt Ứng dụng Solana trên Nano của bạn

- Mở Ledger Live
- Nhấp vào "Manager" ở ngăn bên trái trên ứng dụng và tìm kiếm "Solana" trong Danh mục ứng dụng, sau đó nhấp vào "Install".
  - Đảm bảo rằng thiết bị của bạn được cắm qua USB và được mở khóa bằng mã PIN
- Bạn có thể được nhận được lời nhắc trên Nano để xác nhận cài đặt Ứng dụng Solana
- "Solana" bây giờ sẽ hiển thị là "Installed" trong Trình quản lý Ledger Live

## Nâng cấp lên phiên bản mới nhất của Ứng dụng Solana

Để đảm bảo bạn có chức năng mới nhất, nếu bạn đang sử dụng phiên bản cũ hơn của Ứng dụng Solana, vui lòng nâng cấp lên phiên bản `v1.0.1` bằng cách làm theo các bước sau.

- Đảm bảo bạn có Ledger Live phiên bản 2.10.0 trở lên.
  - Để kiểm tra phiên bản Ledger Live của bạn, hãy nhấp vào nút Cài đặt ở góc trên bên phải, sau đó nhấp vào "About".  Nếu có phiên bản Ledger Live mới hơn, biểu ngữ sẽ nhắc bạn nâng cấp khi bạn mở Ledger Live lần đầu tiên.
- Cập nhật chương trình cơ sở trên Nano của bạn
  - [Cập nhật chương trình cơ sở Nano S](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [Cập nhật chương trình cơ sở Nano X](https://support.ledger.com/hc/en-us/articles/360013349800)
-  Sau khi cập nhật chương trình cơ sở thành công, ứng dụng Solana sẽ tự động được cài đặt lại với phiên bản mới nhất của ứng dụng.

## Tương tác với mạng Solana

Người dùng có thể sử dụng bất kỳ tùy chọn nào sau đây để sử dụng Nano của họ để tương tác với Solana:

- [SolFlare.com](https://solflare.com/) là một ví web không giám sát được xây dựng đặc biệt cho Solana và hỗ trợ các hoạt động chuyển tiền và staking với thiết bị Ledger. Xem hướng dẫn của chúng tôi để [using a Nano with SolFlare](solflare.md).

- Các nhà phát triển và người dùng nâng cao có thể [sử dụng Nano với các công cụ dòng lệnh Solana](hardware-wallets/ledger.md). Các tính năng ví mới hầu như luôn được hỗ trợ trong các công cụ dòng lệnh gốc trước khi được hỗ trợ bởi ví của bên thứ ba.

## Các sự cố Đã biết

- Nano X đôi khi không thể kết nối với ví web sử dụng hệ điều hành Windows. Điều này có thể ảnh hưởng đến các ví sử dụng trên trình duyệt WebUSB. Nhóm Ledger đang làm việc để giải quyết vấn đề này.

## Hỗ trợ

Hãy xem [Trang hỗ trợ Ví](support.md) của chúng tôi để biết cách nhận trợ giúp.
