---
title: Khách hàng Rust
---

## Sự cố

Các bài kiểm tra cấp độ cao, chẳng hạn như bài kiểm tra định kỳ, được viết theo đặc điểm của `Client`. Khi chúng tôi thực hiện các thử nghiệm này như một phần của bộ thử nghiệm, chúng tôi sử dụng triển khai `BankClient` cấp thấp. Khi chúng tôi cần chạy cùng một bài kiểm tra với một cụm, chúng tôi sử dụng `ThinClient`. Vấn đề với cách tiếp cận đó là nó có đặc điểm sẽ liên tục mở rộng để bao gồm các chức năng tiện ích mới và tất cả các triển khai của nó cần phải thêm chức năng mới. Bằng cách tách đối tượng hướng tới người dùng khỏi đặc điểm tóm tắt giao diện mạng, chúng tôi có thể mở rộng đối tượng hướng tới người dùng để bao gồm tất cả các loại chức năng hữu ích, chẳng hạn như "spinner" từ RpcClient mà không cần quan tâm đến việc mở rộng đặc điểm và triển khai của nó.

## Giải pháp Đề xuất

Thay vì triển khai `Client`, `ThinClient` nên được xây dựng với việc triển khai nó. Bằng cách đó, tất cả các chức năng tiện ích hiện có trong `Client` có thể chuyển sang `ThinClient`. `ThinClient` sau đó có thể chuyển sang `solana-sdk` vì tất cả các phụ thuộc mạng của nó sẽ được triển khai trong `Client`. Sau đó, chúng tôi sẽ thêm một triển khai mới của `Client`, được gọi là `ClusterClient` và sẽ tồn tại trong `solana-client`, nơi `ThinClient` đang cư trú.

Sau lần reorg này, bất kỳ mã nào cần một máy khách sẽ được viết dưới dạng `ThinClient`. Trong các bài kiểm tra đơn vị, chức năng sẽ được gọi bằng `ThinClient<BankClient>`, trong `main()` các hàm, điểm chuẩn và kiểm tra tích hợp nó sẽ gọi nó bằng `ThinClient<ClusterClient>`.

Nếu các thành phần cấp cao hơn yêu cầu nhiều chức năng hơn những gì có thể được triển khai bởi `BankClient`, thì nó nên được thực hiện bởi một đối tượng thứ hai thực hiện một đặc điểm thứ hai, theo cùng một mô hình được mô tả ở đây.

### Xử lý Lỗi

`Client` nên sử dụng `TransportError` enum hiện có để tìm lỗi, ngoại trừ các `Custom(String)` cần được thay đổi thành `Custom(Box<dyn Error>)`.

### Chiến lược Thực hiện

1. Thêm đối tượng mới vào `solana-sdk`, `RpcClientTng`, trong đó `Tng` hậu tố là tạm thời và là viết tắt của "Thế hệ tiếp theo"
2. Khởi tạo `RpcClientTng` bằng cách triển khai `SyncClient`.
3. Thêm đối tượng mới vào `solana-sdk`, `ThinClientTng`; khởi tạo nó bằng `RpcClientTng` và triển khai `AsyncClient`
4. Di chuyển tất cả các bài kiểm tra đơn vị từ `BankClient` sang `ThinClientTng<BankClient>`
5. Thêm vào `ClusterClient`
6. Di chuyển `ThinClient` sang `ThinClientTng<ClusterClient>`
7. Xóa `ThinClient` và đổi tên `ThinClientTng` thành `ThinClient`
8. Di chuyển `RpcClient` sang `ThinClient<ClusterClient>`
9. Xóa `RpcClient` và đổi tên `RpcClientTng` thành `RpcClient`
