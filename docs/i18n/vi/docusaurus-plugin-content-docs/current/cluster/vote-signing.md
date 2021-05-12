---
title: Đăng ký biểu quyết an toàn
---

Validator nhận các bài dự thi từ leader hiện tại và gửi phiếu xác nhận các bài viết đó là hợp lệ. Việc gửi phiếu bầu này đưa ra một thách thức bảo mật, vì các phiếu bầu giả mạo vi phạm các quy tắc đồng thuận có thể được sử dụng để cắt stake của validator.

Validator bỏ phiếu fork đã chọn của mình bằng cách gửi một giao dịch sử dụng khóa không đối xứng để ký kết quả xác thực của nó. Các thực thể khác có thể xác minh chữ ký này bằng cách sử dụng public key của validator. Nếu key của validator được sử dụng để ký dữ liệu không chính xác \(e.g. phiếu bầu trên nhiều fork của sổ cái\), stake của node hoặc tài nguyên của nó có thể bị xâm phạm.

Solana giải quyết rủi ro này bằng cách tách ra một dịch vụ _vote signer_ riêng để đánh giá từng phiếu bầu để đảm bảo nó không vi phạm điều khoản slashing.

## Các Validator, các người ký biểu quyết và các bên liên quan

Khi validator nhận nhiều khối cho cùng một slot, nó sẽ theo dõi tất cả các nhánh có thể có cho đến khi xác định được cái "tốt nhất". Validator chọn fork tốt nhất bằng cách gửi phiếu bầu cho nó, sử dụng vote signer để giảm thiểu khả năng phiếu bầu của họ vô tình vi phạm quy tắc đồng thuận và bị cắt stake.

Vote signer do validator đề xuất và chỉ ký vào phiếu bầu nếu nó không vi phạm điều khoản slashing. Vote signer chỉ cần duy trì trạng thái tối thiểu liên quan đến các phiếu mà họ đã ký và các phiếu được ký bởi phần còn lại của cụm. Nó không cần phải xử lý một tập hợp đầy đủ các giao dịch.

Các bên liên quan là danh tính có quyền kiểm soát vốn stake. Bên liên quan có thể ủy thác stake của mình cho vote signer. Sau khi ủy quyền stake, phiếu bầu của vote signers đại diện cho trọng lượng biểu quyết của tất cả các stake được ủy quyền và tạo ra phần thưởng cho tất cả các stake được ủy quyền.

Hiện tại, mối quan hệ 1: 1 giữa các validator và người ký biểu quyết và các bên liên quan ủy thác toàn bộ tiền stake của họ cho một người ký biểu quyết duy nhất.

## Dịch vụ ký

Dịch vụ ký phiếu bầu bao gồm một máy chủ JSON RPC và một bộ xử lý yêu cầu. Khi khởi động, dịch vụ khởi động máy chủ RPC tại một cổng đã cài đặt cấu hình và đợi các yêu cầu của validator. Nó mong đợi các yêu cầu sau:

1. Đăng ký một node validator mới

    - Yêu cầu phải chứa danh tính của validator \(public key\)
    - Yêu cầu phải được ký bằng private key của validator
    - Dịch vụ từ chối yêu cầu nếu không thể xác minh chữ ký của yêu cầu
    - Dịch vụ tạo khóa bất đối xứng biểu quyết mới cho validator và trả về public key dưới dạng phản hồi
    - Nếu validator cố gắng đăng ký lại, dịch vụ sẽ trả về public key từ keypair tồn tại trước đó

2. Ký một phiếu bầu

    - Yêu cầu phải chứa giao dịch biểu quyết và tất cả dữ liệu xác minh
    - Yêu cầu phải được ký bằng private key của validator
    - Dịch vụ từ chối yêu cầu nếu không thể xác minh chữ ký của yêu cầu
    - Dịch vụ xác minh dữ liệu biểu quyết
    - Dịch vụ trả về chữ ký cho giao dịch

## Biểu quyết Validator

Một node validator, khi khởi động, tạo một tài khoản bỏ phiếu mới và đăng ký nó với cụm bằng cách gửi một giao dịch "đăng ký phiếu bầu" mới. Các node khác trên cụm xử lý giao dịch này và bao gồm validator mới trong tập hợp đang hoạt động. Sau đó, validator gửi một giao dịch "bỏ phiếu mới" được ký bằng private key bỏ phiếu của validator trên mỗi sự kiện biểu quyết.

### Cấu hình

Node validator được cài đặt cấu hình với điểm cuối mạng \(IP/Port\) của dịch vụ ký.

### Đăng ký

Khi khởi động, validator tự đăng ký với dịch vụ ký của nó bằng JSON RPC. Lệnh gọi RPC trả về public key bỏ phiếu cho node validator. Validator tạo một giao dịch "đăng ký phiếu bầu" mới bao gồm public key này và gửi nó tới cụm.

### Bình chọn Bộ sưu tập

Validator sẽ tra cứu các phiếu bầu được gửi bởi tất cả các node trong cụm trong khoảng thời gian biểu quyết cuối cùng. Thông tin này được gửi đến dịch vụ ký kết với yêu cầu ký phiếu bầu mới.

### Đăng ký Biểu quyết mới

Validator tạo một giao dịch "phiếu bầu mới" và gửi nó đến dịch vụ ký kết bằng JSON RPC. Yêu cầu RPC cũng bao gồm dữ liệu xác minh phiếu bầu. Khi thành công, lệnh gọi RPC trả về chữ ký cho phiếu bầu. Khi thất bại, lệnh gọi RPC trả về mã lỗi.
