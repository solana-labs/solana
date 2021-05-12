# Lịch sử giao dịch RPC dài hạn
Cần có RPC để phục vụ lịch sử giao dịch ít nhất 6 tháng.  Lịch sử hiện tại, theo thứ tự ngày, không đủ cho người dùng phía dưới.

Dữ liệu giao dịch trong 6 tháng không thể được lưu trữ trên thực tế trong sổ cái của validator, vì vậy cần phải có kho dữ liệu bên ngoài.   Sổ cái stonedb của validator sẽ tiếp tục đóng vai trò là nguồn dữ liệu chính và sau đó sẽ quay trở lại kho dữ liệu bên ngoài.

Các điểm cuối RPC bị ảnh hưởng là:
* [getFirstAvailableBlock](developing/clients/jsonrpc-api.md#getfirstavailableblock)
* [getConfirmedBlock](developing/clients/jsonrpc-api.md#getconfirmedblock)
* [getConfirmedBlocks](developing/clients/jsonrpc-api.md#getconfirmedblocks)
* [getConfirmedSignaturesForAddress](developing/clients/jsonrpc-api.md#getconfirmedsignaturesforaddress)
* [getConfirmedTransaction](developing/clients/jsonrpc-api.md#getconfirmedtransaction)
* [getSignatureStatuses](developing/clients/jsonrpc-api.md#getsignaturestatuses)

Lưu ý rằng [getBlockTime](developing/clients/jsonrpc-api.md#getblocktime) không được hỗ trợ, vì sau khi https://github.com/solana-labs/solana/issues/10089 được sửa thì `getBlockTime` có thể bị loại bỏ.

Một số ràng buộc thiết kế hệ thống:
* Khối lượng dữ liệu cần lưu trữ và tìm kiếm có thể nhanh chóng tăng lên hàng terabyte và là bất biến.
* Hệ thống phải càng nhẹ càng tốt cho các SRE.  Ví dụ, một cụm cơ sở dữ liệu SQL yêu cầu SRE liên tục theo dõi và cân bằng lại các node là điều không mong muốn.
* Dữ liệu phải có thể tìm kiếm được trong thời gian thực - không thể chấp nhận các truy vấn hàng loạt mất vài phút hoặc hàng giờ để chạy.
* Dễ dàng sao chép dữ liệu trên toàn thế giới để đồng định vị dữ liệu đó với các điểm cuối RPC sẽ sử dụng nó.
* Giao tiếp với kho dữ liệu bên ngoài sẽ dễ dàng và không yêu cầu phụ thuộc vào các thư viện mã được cộng đồng hỗ trợ ít rủi ro

Dựa trên những ràng buộc này, sản phẩm BigTable của Google được chọn làm nơi lưu trữ dữ liệu.

## Lược đồ Bảng
Một phiên bản BigTable được sử dụng để chứa tất cả dữ liệu giao dịch, được chia thành các bảng khác nhau để tìm kiếm nhanh chóng.

Dữ liệu mới có thể được sao chép vào phiên bản bất kỳ lúc nào mà không ảnh hưởng đến dữ liệu hiện có và tất cả dữ liệu là bất biến.  Nói chung, kỳ vọng là dữ liệu mới sẽ được tải lên sau khi kỷ nguyên hiện tại hoàn tất nhưng không có giới hạn về tần suất kết xuất dữ liệu.

Việc dọn dẹp dữ liệu cũ là tự động bằng cách định cấu hình chính sách lưu giữ dữ liệu của các bảng cá thể một cách thích hợp, nó chỉ biến mất.  Do đó, thứ tự khi dữ liệu được thêm vào trở nên quan trọng.  Ví dụ: nếu dữ liệu từ kỷ nguyên N-1 được thêm vào sau dữ liệu từ kỷ nguyên N, thì dữ liệu kỷ nguyên cũ hơn sẽ tồn tại lâu hơn dữ liệu mới hơn.  Tuy nhiên, ngoài việc tạo ra _các lỗ hổng_ trong kết quả truy vấn, loại xóa không theo thứ tự này sẽ không có tác dụng xấu.  Lưu ý rằng phương pháp dọn dẹp này một cách hiệu quả cho phép lưu trữ số lượng dữ liệu giao dịch không giới hạn, chỉ bị hạn chế bởi chi phí tiền tệ của việc làm như vậy.

Bố cục bảng chỉ hỗ trợ các điểm cuối RPC hiện có.  Các điểm cuối RPC mới trong tương lai có thể yêu cầu bổ sung vào lược đồ và có khả năng lặp lại trên tất cả các giao dịch để tạo siêu dữ liệu cần thiết.

## Truy cập BigTable
BigTable có điểm cuối gRPC có thể được truy cập bằng cách sử dụng [thuốc bổ](https://crates.io/crates/crate)] và API protobuf thô, vì hiện tại không tồn tại thùng Rust cấp cao hơn cho BigTable.  Trên thực tế, điều này làm cho việc phân tích kết quả của các truy vấn BigTable phức tạp hơn nhưng không phải là một vấn đề đáng kể.

## Dữ liệu dân số
Dân số liên tục của dữ liệu phiên bản sẽ xảy ra theo nhịp kỷ nguyên thông qua việc sử dụng một lệnh mới `solana-ledger-tool` sẽ chuyển đổi dữ liệu stonedb cho một phạm vi slot nhất định thành lược đồ cá thể.

Quá trình tương tự sẽ được chạy một lần, theo cách thủ công, để chèn lấp dữ liệu sổ cái hiện có.

### Bảng khối: `block`

Bảng này chứa dữ liệu khối được nén cho một slot nhất định.

Khóa hàng được tạo bằng cách lấy biểu diễn thập lục phân viết thường gồm 16 chữ số của slot, để đảm bảo rằng slot cũ nhất có khối đã được xác nhận sẽ luôn ở vị trí đầu tiên khi các hàng được liệt kê.  ví dụ: Khóa hàng cho slot 42 sẽ là 000000000000002a.

Dữ liệu hàng là một cấu trúc `StoredConfirmedBlock` được nén.


### Bảng tra cứu chữ ký giao dịch địa chỉ tài khoản: `tx-by-addr`

Bảng này chứa các giao dịch ảnh hưởng đến một địa chỉ nhất định.

Khóa hàng là `<base58
address>/<slot-id-one's-compliment-hex-slot-0-prefixed-to-16-digits>`.  Dữ liệu hàng là một cấu trúc `TransactionByAddrInfo` được nén.

Nhận được lời khen ngợi của một người về các slot cho phép danh sách các slot đảm bảo rằng slot mới nhất có các giao dịch ảnh hưởng đến địa chỉ sẽ luôn được liệt kê đầu tiên.

Địa chỉ Sysvar không được lập chỉ mục.  Tuy nhiên, các chương trình được sử dụng thường xuyên như Bỏ phiếu hoặc Hệ thống, và có thể sẽ có một hàng cho mọi slot được xác nhận.

### Bảng tra cứu chữ ký giao dịch: `tx`

Bảng này ánh xạ chữ ký giao dịch tới khối đã xác nhận của nó và lập chỉ mục trong khối đó.

Khóa hàng là chữ ký giao dịch được mã hóa base58. Dữ liệu hàng là một cấu trúc `TransactionInfo` được nén.
