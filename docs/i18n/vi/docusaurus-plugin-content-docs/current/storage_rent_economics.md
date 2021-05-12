---
title: Kinh tế thuê lưu trữ
---

Mỗi giao dịch được gửi đến sổ cái Solana sẽ chịu chi phí. Về lý thuyết, phí giao dịch do người gửi trả và do validator thu, tính cho các chi phí giao dịch của việc xác thực và thêm dữ liệu đó vào sổ cái. Không tính toán trong quá trình này là lưu trữ trung hạn của trạng thái sổ cái đang hoạt động, nhất thiết phải được duy trì bởi validator luân phiên. Loại lưu trữ này không chỉ đặt ra chi phí đối với các validator mà còn đối với mạng rộng hơn khi trạng thái hoạt động phát triển cũng như chi phí truyền dữ liệu và xác thực. Để giải thích cho những chi phí này, chúng tôi mô tả ở đây thiết kế sơ bộ của chúng tôi và việc thực hiện tiền thuê kho.

Có thể thanh toán tiền thuê kho bằng một trong hai hình thức:

Phương pháp 1: Đặt nó và quên nó

Với cách tiếp cận này, các tài khoản bảo đảm tiền thuê nhà trị giá hai năm sẽ được miễn phí thuê mạng. Bằng cách duy trì số dư tối thiểu này, mạng lưới rộng lớn hơn được hưởng lợi từ việc giảm tính thanh khoản và chủ tài khoản có thể tin tưởng rằng `Account::data` của họ sẽ được giữ lại để truy cập/sử dụng liên tục.

Phương pháp 2: Thanh toán cho mỗi byte

Nếu một tài khoản có giá trị thuê ít hơn hai năm, mạng sẽ tính tiền thuê trên cơ sở mỗi kỷ nguyên, bằng tín dụng cho kỷ nguyên tiếp theo. Tiền thuê này được khấu trừ theo tỷ lệ quy định ban đầu, tính bằng số lượng lamport trên kilobyte-năm.

Để biết thêm thông tin về chi tiết triển khai kỹ thuật của thiết kế này, hãy xem phần [Thuê](implemented-proposals/rent.md).
