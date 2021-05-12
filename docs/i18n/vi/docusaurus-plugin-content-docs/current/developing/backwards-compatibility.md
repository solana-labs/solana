---
title: Chính sách tương thích ngược
---

Khi hệ sinh thái Solana phát triển, thì cũng cần có những kỳ vọng rõ ràng xung quanh việc phá vỡ các thay đổi về API và hành vi ảnh hưởng đến các ứng dụng và công cụ được xây dựng cho Solana. Trong một thế giới hoàn hảo, quá trình phát triển Solana có thể tiếp tục với tốc độ rất nhanh mà không gây ra vấn đề gì cho các nhà phát triển hiện tại. Tuy nhiên, một số thỏa hiệp sẽ cần được thực hiện và vì vậy tài liệu này cố gắng làm rõ và hệ thống hóa quy trình cho các bản phát hành mới.

### Các kỳ vọng

- Các bản phát hành phần mềm Solana bao gồm API, SDK và công cụ CLI (với một số [ngoại lệ](#exceptions)).
- Các bản phát hành phần mềm Solana tuân theo cách lập phiên bản ngữ nghĩa, thêm chi tiết bên dưới.
- Phần mềm cho bản phát hành phiên bản `MINOR` sẽ tương thích với tất cả các phần mềm trên cùng một phiên bản `MAJOR`.

### Quy trình ngừng sử dụng

1. Trong bất kỳ bản phát hành `PATCH` hoặc `MINOR` nào, một tính năng, API, điểm cuối, v. v. có thể được đánh dấu là không dùng nữa.
2. Theo độ khó nâng cấp mã, một số tính năng sẽ không được dùng nữa trong một vài chu kỳ phát hành.
3. Trong bản phát hành `MAJOR` trong tương lai, các tính năng không dùng nữa sẽ bị loại bỏ theo cách không tương thích.

### Phát hành nhịp điệu

Solana RPC API, Rust SDK, CLI tooling và BPF Program SDK đều được cập nhật và vận chuyển cùng với mỗi bản phát hành phần mềm Solana và phải luôn tương thích giữa các bản cập nhật `PATCH` của một `MINOR` phiên bản phát hành.

#### Phát hành kênh

- `edge` phần mềm có chứa các tính năng tiên tiến mà không có chính sách tương thích ngược
- `beta` phần mềm chạy trên cụm testnet Solana Tour de SOL
- `stable` phần mềm chạy trên cụm Mainnet Beta và Devnet Solana

#### Bản phát hành chính (x.0.0)

`MAJOR` phiên bản phát hành (ví dụ: 2.0.0) có thể chứa các thay đổi đột phá và loại bỏ các tính năng không dùng nữa trước đây. SDK khách hàng và công cụ sẽ bắt đầu sử dụng các tính năng và điểm cuối mới đã được bật trong phiên bản `MAJOR` trước đó.

#### Bản phát hành nhỏ (1.x.0)

Các tính năng mới và triển khai đề xuất được thêm vào _mới_ `MINOR` phiên bản phát hành (ví dụ: 1.4.0) và lần đầu tiên được chạy trên cụm testnet Tour de SOL của Solana. Trong khi chạy trên testnet, các phiên bản `MINOR` được coi là nằm trong kênh phát hành `beta`. Sau khi những thay đổi đó đã được vá khi cần thiết và được chứng minh là đáng tin cậy, phiên bản `MINOR` sẽ được nâng cấp lên kênh phát hành `ổn định` và triển khai cho cụm Mainnet Beta.

#### Bản vá lỗi (1.0.x)

Các tính năng rủi ro thấp, các thay đổi không vi phạm cũng như các bản sửa lỗi và bảo mật được vận chuyển như một phần của phiên bản phát hành `PATCH` (ví dụ: 1.0.11). Các bản vá lỗi có thể được áp dụng cho cả hai kênh phát hành `beta` và `ổn định`.

### RPC API

Bản vá lỗi:

- Sửa lỗi
- Các bản sửa lỗi bảo mật
- Điểm cuối / tính năng không được dùng nữa

Bản phát hành nhỏ:

- Các tính năng và điểm cuối RPC mới

Các bản phát hành chính:

- Loại bỏ các tính năng không dùng nữa

### Thùng Rust

- [`solana-sdk`](https://docs.rs/solana-sdk/) - SDK Rust để tạo các giao dịch và phân tích trạng thái tài khoản
- [`solana-program`](https://docs.rs/solana-program/) - Rust SDK để viết các chương trình
- [`solana-client`](https://docs.rs/solana-client/) - Khách hàng Rust để kết nối với API RPC
- [`solana-cli-config`](https://docs.rs/solana-cli-config/) - Khách hàng Rust để quản lý tệp cấu hình Solana CLI

Bản vá lỗi:

- Sửa lỗi
- Các bản sửa lỗi bảo mật
- Các cải tiến hiệu suất

Bản phát hành nhỏ:

- Các API mới

Các bản phát hành chính

- Loại bỏ các API không dùng nữa
- Ngược lại các thay đổi hành vi không tương thích

### Các công cụ CLI

Bản vá lỗi:

- Các bản sửa lỗi và bảo mật
- Các cải tiến hiệu suất
- Lệnh con / đối số không được chấp nhận

Bản phát hành nhỏ:

- Lệnh con mới

Các bản phát hành chính:

- Chuyển sang cấu hình / điểm cuối API RPC mới được giới thiệu trong phiên bản chính trước đó.
- Loại bỏ các tính năng không dùng nữa

### Tính năng thời gian chạy

Các tính năng thời gian chạy Solana mới được chuyển đổi tính năng và kích hoạt thủ công. Các tính năng thời gian chạy bao gồm: giới thiệu các chương trình gốc, sysvars, và syscalls mới; và những thay đổi đối với hành vi của họ. Kích hoạt tính năng là bất khả tri theo cụm, cho phép sự tự tin được xây dựng trên Testnet trước khi kích hoạt trên Mainnet-beta.

Quá trình phát hành như sau:

1. Tính năng thời gian chạy mới được bao gồm trong một bản phát hành mới, bị vô hiệu hóa theo mặc định
2. Sau khi đủ các validator được stake nâng cấp lên bản phát hành mới, công tắc tính năng thời gian chạy được kích hoạt theo cách thủ công với một hướng dẫn
3. Tính năng này có hiệu lực vào đầu kỷ nguyên tiếp theo

### Thay đổi cơ sở hạ tầng

#### Các node API công khai

Solana cung cấp các node API RPC có sẵn công khai cho tất cả các nhà phát triển sử dụng. Nhóm Solana sẽ nỗ lực hết sức để thông báo bất kỳ thay đổi nào đối với máy chủ, cổng, hành vi giới hạn tốc độ, tính khả dụng, v. v. Tuy nhiên, chúng tôi khuyên các nhà phát triển nên dựa vào các validator node của riêng họ để không phụ thuộc vào các node do Solana điều hành.

#### Tập lệnh cụm cục bộ và hình ảnh Docker

Các thay đổi đột phá sẽ được giới hạn ở các bản cập nhật phiên bản `MAJOR`. Các bản cập nhật `MINOR` và `PATCH` phải luôn tương thích ngược.

### Các ngoại lệ

#### Web3 JavaScript SDK

Web3.JS SDK cũng tuân theo các thông số kỹ thuật lập phiên bản ngữ nghĩa nhưng được phân phối riêng với các bản phát hành phần mềm Solana.

#### Tấn công các Vectơ

Nếu tấn công vectơ mới được phát hiện trong code hiện có, các quy trình trên có thể bị phá vỡ để nhanh chóng triển khai bản sửa lỗi, tùy thuộc vào mức độ nghiêm trọng của vấn đề.
