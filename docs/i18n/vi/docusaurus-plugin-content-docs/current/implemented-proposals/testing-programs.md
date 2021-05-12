---
title: Chương trình thử nghiệm
---

Các ứng dụng gửi các giao dịch đến một cụm Solana và truy vấn các validator để xác nhận các giao dịch đã được xử lý và kiểm tra kết quả của từng giao dịch. Khi cụm không hoạt động như dự đoán, có thể do số lý do sau:

- Chương trình có lỗi
- Bộ tải BPF đã từ chối một chỉ dẫn chương trình không an toàn
- Giao dịch quá lớn
- Giao dịch không hợp lệ
- Thời gian chạy cố gắng thực hiện giao dịch khi một người khác đang truy cập

  cùng một tài khoản

- Mạng đã bỏ giao dịch
- Cụm đảo ngược sổ cái
- Validator đã phản hồi các truy vấn một cách ác ý

## Các đặc điểm của AsyncClient và SyncClient

Để khắc phục sự cố, ứng dụng nên nhắm mục tiêu lại một thành phần cấp thấp hơn, nơi có thể có ít lỗi hơn. Nhắm mục tiêu lại có thể được thực hiện với các triển khai khác nhau của các đặc điểm AsyncClient và SyncClient.

Thành phần thực hiện các phương pháp chính:

```text
trait AsyncClient {
    fn async_send_transaction(&self, transaction: Transaction) -> io::Result<Signature>;
}

trait SyncClient {
    fn get_signature_status(&self, signature: &Signature) -> Result<Option<transaction::Result<()>>>;
}
```

Người dùng gửi các giao dịch và chờ đợi kết quả đồng bộ và không đồng bộ.

### ThinClient cho Cụm

Việc thực hiện cấp cao nhất, ThinClient, nhắm mục tiêu đến một cụm Solana, có thể là một mạng thử nghiệm được triển khai hoặc một cụm cục bộ chạy trên một máy phát triển.

### TpuClient cho TPU

Cấp độ tiếp theo là triển khai TPU, vẫn chưa được thực hiện. Ở cấp độ TPU, ứng dụng gửi các giao dịch qua các kênh Rust, nơi không thể có bất ngờ từ hàng đợi mạng hoặc các gói tin bị rớt. TPU thực hiện tất cả các lỗi giao dịch "normal". Nó thực hiện xác minh chữ ký, có thể báo cáo lỗi sử dụng tài khoản, và nếu không sẽ dẫn đến sổ cái, hoàn chỉnh với bằng chứng về lịch sử băm.

## Thử nghiệm cấp độ thấp

### BankClient cho Ngân hàng

Dưới mức TPU là Ngân hàng. Ngân hàng không thực hiện xác minh chữ ký hoặc tạo sổ cái. Ngân hàng là một tầng thuận tiện để thử nghiệm các chương trình mới trên chuỗi. Nó cho phép các nhà phát triển chuyển đổi giữa việc triển khai chương trình gốc và các biến thể do BPF biên dịch. Không cần đặc điểm Giao dịch ở đây. API của Ngân hàng là đồng bộ.

## Kiểm tra đơn vị với Thời gian chạy

Bên dưới Ngân hàng là Thời gian chạy. Thời gian chạy là môi trường thử nghiệm lý tưởng để thử nghiệm đơn vị. Bằng cách liên kết tĩnh Runtime với triển khai chương trình gốc, nhà phát triển đạt được edit-compile-run loop ngắn nhất có thể. Không có bất kỳ liên kết động nào, dấu vết ngăn xếp bao gồm các ký hiệu gỡ lỗi và lỗi chương trình có thể dễ dàng khắc phục sự cố.
