---
title: Trình xác thực
---

## Lịch sử

Khi chúng tôi lần đầu tiên bắt đầu Solana, mục tiêu là giảm rủi ro cho các yêu cầu TPS của chúng tôi. Chúng tôi biết rằng giữa kiểm soát đồng thời lạc quan và các leader slot đủ dài, thì sự đồng thuận PoS không phải là rủi ro lớn nhất đối với TPS. Đó là xác minh chữ ký dựa trên GPU, kết nối phần mềm và ngân hàng đồng thời. Như vậy, TPU đã ra đời. Sau khi đạt được 100k TPS, chúng tôi chia nhóm thành một nhóm làm việc hướng tới 710k TPS và một TPS khác để xác thực đường dẫn validator. Do đó, TVU ra đời. Kiến trúc hiện tại là kết quả của sự phát triển gia tăng với thứ tự và các ưu tiên của dự án. Nó không phải là sự phản ánh những gì chúng ta từng tin là mặt cắt kỹ thuật tao nhã nhất của những công nghệ đó. Trong bối cảnh luân chuyển leader, sự khác biệt rõ ràng giữa dẫn đầu và xác thực bị mờ.

## Sự khác biệt giữa xác thực và dẫn đầu

Sự khác biệt cơ bản giữa các đường ống là khi PoH có mặt. Trong một Leader, chúng tôi xử lý các giao dịch, loại bỏ các giao dịch xấu và sau đó gắn thẻ kết quả bằng một hàm băm PoH. Trong validator, chúng tôi xác minh hàm băm đó, bóc nó ra và xử lý các giao dịch theo cùng một cách. Sự khác biệt duy nhất là nếu một validator nhận thấy một giao dịch xấu, không thể đơn giản loại bỏ nó như leader đã làm, bởi vì điều đó sẽ khiến hàm băm PoH thay đổi. Thay vào đó, nó từ chối toàn bộ khối. Sự khác biệt khác giữa các đường ống là những gì xảy ra _sau_ ngân hàng. Leader truyền phát các mục nhập đến các validator hạ lưu trong khi validator sẽ thực hiện điều đó trong RetransmitStage, đây là một tối ưu hóa thời gian xác nhận. Mặt khác, quy trình xác thực còn một bước cuối cùng. Bất cứ khi nào nó xử lý xong một khối, nó cần cân nhắc bất kỳ fork nào mà nó đang quan sát, có thể bỏ phiếu và nếu vậy, hãy đặt lại hàm băm PoH của nó thành hàm băm khối mà nó vừa bỏ phiếu.

## Thiết kế đề xuất

Chúng tôi mở nhiều các layer trừu tượng và xây dựng một đường dẫn duy nhất có thể bật chế độ leader bất cứ khi nào ID của validator hiển thị trong lịch trình leader.

![Sơ đồ khối validator](/img/validator-proposal.svg)

## Những thay đổi đáng chú ý

- Lấy FetchStage và BroadcastStage ra khỏi TPU
- BankForks được đổi tên thành Banktree
- TPU chuyển sang thùng socket-free mới được gọi là solana-tpu.
- TPU's BankingStage hấp thụ ReplayStage
- TVU biến mất
- RepairStage mới hấp thụ Shred Fetch Stage và các yêu cầu sửa chữa
- Dịch vụ JSON RPC là tùy chọn - được sử dụng để gỡ lỗi. Thay vào đó, nó phải là một phần của tệp thực thi `solana-blockstreamer` riêng biệt.
- MulticastStage mới hấp thụ một phần truyền lại của RetransmitStage
- MulticastStage hạ lưu của Blockstore
