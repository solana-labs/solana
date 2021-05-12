---
title: Điểm chuẩn cho một Cụm
---

Kho lưu trữ git Solana chứa tất cả các tập lệnh mà bạn có thể cần để tạo testnet cục bộ của riêng mình. Tùy thuộc vào những gì bạn muốn đạt được, bạn có thể muốn chạy một biến thể khác, vì testnet nhiều node chính thức, nâng cao hiệu suất phức tạp hơn đáng kể để thiết lập so với testnode một node, chỉ dành cho Rust. Nếu bạn đang tìm cách phát triển các tính năng cấp cao, chẳng hạn như thử nghiệm với các hợp đồng thông minh, hãy tiết kiệm cho mình một số vấn đề đau đầu khi thiết lập và tuân theo bản demo singlenode chỉ dành cho Rust. Nếu bạn đang thực hiện tối ưu hóa hiệu suất của quy trình giao dịch, hãy xem xét bản demo singlenode nâng cao. Nếu bạn đang thực hiện công việc đồng thuận, bạn sẽ cần ít nhất một bản demo multinode chỉ có Rust. Nếu bạn muốn tái tạo các chỉ số TPS của chúng tôi, hãy chạy bản trình diễn multinode nâng cao.

Đối với tất cả bốn biến thể, bạn cần chuỗi công cụ Rust mới nhất và mã nguồn Solana:

Trước tiên, hãy thiết lập các gói Rust, Cargo và hệ thống như được mô tả trong Solana [README](https://github.com/solana-labs/solana#1-install-rustc-cargo-and-rustfmt)

Bây giờ hãy kiểm tra mã từ github:

```bash
git clone https://github.com/solana-labs/solana.git
cd solana
```

Code demo đôi khi bị hỏng giữa các bản phát hành vì chúng tôi thêm các tính năng cấp thấp mới, vì vậy nếu đây là lần đầu tiên bạn chạy bản demo, bạn sẽ cải thiện tỷ lệ thành công nếu xem [bản phát hành mới nhất](https://github.com/solana-labs/solana/releases) trước khi tiếp tục:

```bash
TAG=$(git describe --tags $(git rev-list --tags --max-count=1))
git checkout $TAG
```

### Thiết lập cấu hình

Đảm bảo các chương trình quan trọng như chương trình bỏ phiếu được xây dựng trước khi bất kỳ node nào được bắt đầu. Lưu ý rằng chúng tôi đang sử dụng phiên bản phát hành ở đây để có hiệu suất tốt. Nếu bạn muốn xây dựng gỡ lỗi, chỉ sử dụng `cargo build` và bỏ qua phần `NDEBUG=1` của lệnh.

```bash
cargo build --release
```

Mạng được khởi tạo bằng sổ cái gốc được tạo bằng cách chạy tập lệnh sau.

```bash
NDEBUG=1 ./multinode-demo/setup.sh
```

### Vòi

Để các validator và các khách hàng hoạt động, chúng tôi sẽ cần xoay một vòi để đưa ra một số mã thông báo thử nghiệm. Vòi phân phối các "air drop" theo phong cách Milton Friedman ( mã thông báo miễn phí cho khách hàng yêu cầu) để được sử dụng trong các giao dịch thử nghiệm.

Bắt đầu vòi với:

```bash
NDEBUG=1 ./multinode-demo/faucet.sh
```

### Testnet Singlenode

Trước khi bạn bắt đầu một validator, hãy đảm bảo rằng bạn biết địa chỉ IP của máy bạn muốn làm validator bootstrap cho bản demo và đảm bảo rằng các cổng udp 8000-10000 được mở trên tất cả các máy bạn muốn kiểm tra.

Bây giờ hãy khởi động validator bootstrap trong một trình bao riêng:

```bash
NDEBUG=1 ./multinode-demo/bootstrap-validator.sh
```

Chờ một vài giây để máy chủ khởi tạo. Nó sẽ in "leader sẵn sàng..." khi nó sẵn sàng nhận giao dịch. Leader sẽ yêu cầu một số mã thông báo từ vòi nếu nó không có. Vòi không cần phải chạy cho những lần bắt đầu leader tiếp theo.

### Testnet Multinode

Để chạy testnet multinod, sau khi bắt đầu một node leader, hãy quay thêm một số validator bổ sung trong các trình bao riêng biệt:

```bash
NDEBUG=1 ./multinode-demo/validator-x.sh
```

Để chạy validator nâng cao hiệu suất trên Linux, [CUDA 10.0](https://developer.nvidia.com/cuda-downloads) phải được cài đặt trên hệ thống của bạn:

```bash
./fetch-perf-libs.sh
NDEBUG=1 SOLANA_CUDA=1 ./multinode-demo/bootstrap-validator.sh
NDEBUG=1 SOLANA_CUDA=1 ./multinode-demo/validator.sh
```

### Bản demo khách hàng Testnet

Bây giờ, singlenode hoặc testnet multinode của bạn đã hoạt động và hãy gửi cho nó một số giao dịch!

Trong một shell riêng biệt, hãy bắt đầu khách hàng:

```bash
NDEBUG=1 ./multinode-demo/bench-tps.sh # runs against localhost by default
```

Chuyện gì vừa xảy ra? Bản demo khách hàng quay một số luồng để gửi 500,000 giao dịch đến testnet nhanh nhất có thể. Sau đó, khách hàng sẽ ping testnet định kỳ để xem nó đã xử lý bao nhiêu giao dịch trong thời gian đó. Hãy lưu ý rằng bản demo cố tình làm ngập mạng bằng các gói UDP, do đó mạng gần như chắc chắn sẽ giảm một loạt các gói đó. Điều này đảm bảo testnet có cơ hội đạt tới 710k TPS. Bản demo khách hàng hoàn thành sau khi nó đã tự thuyết phục rằng testnet sẽ không xử lý bất kỳ giao dịch bổ sung nào. Bạn sẽ thấy một số phép đo TPS được in ra màn hình. Trong biến thể multinode, bạn cũng sẽ thấy các phép đo TPS cho từng node validator.

### Gỡ lỗi Testnet

Có một số thông báo gỡ lỗi hữu ích trong code, bạn có thể bật chúng trên cơ sở mỗi mô-đun và mỗi-cấp. Trước khi chạy leader hoặc validator hãy đặt biến môi trường RUST_LOG bình thường.

Ví dụ

- Để bật `thông tin` ở mọi nơi và `gỡ lỗi` chỉ trong mô-đun solana::banking_stage:

  ```bash
  export RUST_LOG=solana=info,solana::banking_stage=debug
  ```

- Để bật ghi nhật ký chương trình BPF:

  ```bash
  export RUST_LOG=solana_bpf_loader=trace
  ```

Nói chung, chúng tôi đang sử dụng `debug` cho các tin nhắn gỡ lỗi không thường xuyên, `trace` cho các tin nhắn có khả năng xảy ra thường xuyên và `thông tin` để ghi nhật ký liên quan đến hiệu suất.

Bạn cũng có thể đính kèm vào một quy trình đang chạy với GDB. Quy trình của leader được đặt tên là _solana-validator_:

```bash
sudo gdb
attach <PID>
set logging on
thread apply all bt
```

Điều này sẽ kết xuất tất cả các dấu vết ngăn xếp luồng vào gdb.txt

## Nhà phát triển Testnet

Trong ví dụ này, khách hàng kết nối với testnet công khai của chúng tôi. Để chạy các validator trên testnet, bạn cần mở các cổng udp `8000-10000`.

```bash
NDEBUG=1 ./multinode-demo/bench-tps.sh --entrypoint devnet.solana.com:8001 --faucet devnet.solana.com:9900 --duration 60 --tx_count 50
```

Bạn có thể quan sát tác động của các giao dịch của khách hàng trên [bảng điều khiển số liệu](https://metrics.solana.com:3000/d/monitor/cluster-telemetry?var-testnet=devnet)
