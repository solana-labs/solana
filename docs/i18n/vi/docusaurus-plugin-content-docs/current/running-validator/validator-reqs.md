---
title: Yêu cầu về Validator
---

## Phần cứng

- CPU khuyến nghị
  - Chúng tôi khuyên bạn nên sử dụng CPU có số lõi cao nhất có thể. CPU AMD Threadripper hoặc Intel Server \(Xeon\) đều ổn.
  - Chúng tôi khuyên bạn nên sử dụng AMD Threadripper vì bạn nhận được số lượng lõi lớn hơn để chạy song song so với Intel.
  - Threadripper cũng có lợi thế về chi phí trên mỗi lõi và số lượng làn PCIe nhiều hơn so với Intel tương đương. PoH \(Proof of History\) dựa trên sha256 và Threadripper cũng hỗ trợ các hướng dẫn phần cứng của sha256.
- Kích thước SSD và kiểu I/O \(SATA vs NVMe /M.2\) cho validator
  - Ví dụ tối thiểu - Samsung 860 Evo 2TB
  - Ví dụ tầm trung - Samsung 860 Evo 4TB
  - Ví dụ cao cấp - Samsung 860 Evo 4TB
- GPUs
  - Mặc dù node chỉ dành cho CPU có thể theo kịp với mạng chạy không tải ban đầu, nhưng khi thông lượng giao dịch tăng lên, GPU sẽ là cần thiết
  - Loại GPU nào?
    - Chúng tôi đề xuất GPU dòng Nvidia Turing và dòng volta 1660ti đến 2080ti dòng tiêu dùng hoặc GPU máy chủ dòng Tesla.
    - Chúng tôi hiện không hỗ trợ OpenCL và do đó không hỗ trợ GPU AMD. Chúng tôi có một khoản tiền thưởng để ai đó chuyển chúng tôi sang OpenCL. Thú vị? [Kiểm tra GitHub.](https://github.com/solana-labs/solana)
- Sự tiêu thụ năng lượng
  - Mức tiêu thụ điện năng gần đúng cho một validator chạy GPU AMD Threadripper 3950x và 2x 2080Ti là 800-1000W.

### Các thiết lập được cấu hình sẵn

Dưới đây là các đề xuất của chúng tôi về các thông số kỹ thuật của máy thấp, trung bình và cao cấp:

|                   | Thấp                | Trung bình             | Cao                    | Ghi chú                                                                                   |
|:----------------- |:------------------- |:---------------------- |:---------------------- |:----------------------------------------------------------------------------------------- |
| CPU               | AMD Ryzen 3950x     | AMD Threadripper 3960x | AMD Threadripper 3990x | Hãy xem xét một bo mạch chủ hỗ trợ 10Gb với càng nhiều rãnh PCIe và khe cắm m.2 càng tốt. |
| RAM               | 32GB                | 64GB                   | 128GB                  |                                                                                           |
| Ledger Drive      | Samsung 860 Evo 2TB | Samsung 860 Evo 4TB    | Samsung 860 Evo 4TB    | Hoặc SSD tương đương                                                                      |
| Accounts Drive(s) | Không có            | Samsung 970 Pro 1TB    | 2x Samsung 970 Pro 1TB |                                                                                           |
| GPU               | Nvidia 1660ti       | Nvidia 2080 Ti         | 2x Nvidia 2080 Ti      | Bất kỳ số lượng GPU có khả năng cuda nào đều được hỗ trợ trên nền tảng Linux.             |

## Máy ảo trên Nền tảng đám mây

Mặc dù bạn có thể chạy validator trên nền tảng điện toán đám mây, nhưng nó có thể không tiết kiệm chi phí về lâu dài.

Tuy nhiên, có thể thuận tiện để chạy các node api không bỏ phiếu trên các phiên bản VM để sử dụng nội bộ của riêng bạn. Trường hợp sử dụng này bao gồm các sàn giao dịch và dịch vụ được xây dựng trên Solana.

Trên thực tế, các node API mainnet-beta chính thức hiện đang (tháng 10, năm 2020) chạy trên các phiên bản GCE `n1-standard-32` (32 vCPU, bộ nhớ 120 GB) với SSD 2048 GB để thuận tiện cho hoạt động.

Đối với các nền tảng đám mây khác, hãy chọn các loại phiên bản có thông số kỹ thuật tương tự.

Cũng lưu ý rằng việc sử dụng lưu lượng truy cập internet đầu ra có thể cao, đặc biệt là đối với trường hợp đang chạy các validator stake.

## Docker

Việc chạy validator cho các cụm trực tiếp (bao gồm cả mainnet-beta) bên trong Docker không được khuyến nghị và thường không được hỗ trợ. Điều này là do lo ngại về tổng chi phí lưu trữ của docker chung và dẫn đến sự suy giảm hiệu suất trừ khi được cấu hình đặc biệt.

Chúng tôi chỉ sử dụng docker cho mục đích phát triển.

## Phần mềm

- Chúng tôi xây dựng và chạy trên Ubuntu 18.04. Một số người dùng đã gặp sự cố khi chạy trên Ubuntu 16.04
- Xem [Cài đặt Solana](../cli/install-solana-cli-tools.md) để biết bản phát hành phần mềm Solana hiện tại.

Hãy đảm bảo rằng máy được sử dụng không có NAT dân dụng để tránh các vấn đề về truyền tải NAT. Máy được lưu trữ trên đám mây hoạt động tốt nhất. **Đảm bảo rằng các cổng IP từ 8000 đến 10000 không bị chặn đối với lưu lượng truy cập Internet vào và ra.** Để biết thêm thông tin về chuyển tiếp cổng liên quan đến mạng dân cư, xem [tài liệu này](http://www.mcs.sdsmt.edu/lpyeatt/courses/314/PortForwardingSetup.pdf).

Các tệp nhị phân dựng sẵn có sẵn cho Linux x86_64 \(nên dùng Ubuntu 18.04 \). Người dùng MacOS hoặc WSL có thể xây dựng từ nguồn.

## GPU yêu cầu

CUDA là cần thiết để sử dụng GPU trên hệ thống của bạn. Các tệp nhị phân phát hành Solana được cung cấp được xây dựng trên Ubuntu 18.04 với [CUDA Toolkit 10.1 update 1](https://developer.nvidia.com/cuda-toolkit-archive). Nếu máy của bạn đang sử dụng phiên bản CUDA khác thì bạn cần phải xây dựng lại từ nguồn.
