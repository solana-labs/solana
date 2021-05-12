---
title: "Tài khoản"
---

## Lưu trữ trạng thái giữa các giao dịch

Nếu chương trình cần lưu trữ trạng thái giữa các giao dịch, chương trình sẽ sử dụng các _tài khoản_. Tài khoản tương tự như các tệp trong hệ điều hành như Linux. Giống như một tệp, tài khoản có thể giữ dữ liệu tùy ý và dữ liệu đó tồn tại ngoài vòng đời của chương trình. Cũng giống như tệp, tài khoản bao gồm siêu dữ liệu thời gian chạy cho biết ai được phép truy cập dữ liệu và cách thức.

Không giống như một tệp, tài khoản bao gồm siêu dữ liệu cho thời gian tồn tại của tệp. Thời gian tồn tại đó được biểu thị bằng các "mã thông báo", là một số mã thông báo gốc theo phân số, được gọi là các _lamport_. Các tài khoản được giữ trong bộ nhớ của validator và trả tiền ["thuê"](#rent) để ở đó. Mỗi validator định kỳ quét tất cả các tài khoản và thu tiền thuê. Bất kỳ tài khoản nào giảm xuống 0 lamport sẽ bị xóa.  Các tài khoản cũng có thể được đánh dấu là [miễn-tiền thuê](#rent-exemption) nếu chúng có chứa đủ số lượng lamport.

Giống như cách người dùng Linux sử dụng đường dẫn để tra cứu tệp, một khách hàng Solana sử dụng một _địa chỉ_ để tra cứu tài khoản. Địa chỉ là một public key 256 bit.

## Người ký tên

Các giao dịch có thể bao gồm các [chữ ký](terminology.md#signature) điện tử tương ứng với các public key của tài khoản được tham chiếu bởi giao dịch. Khi có chữ ký điện tử tương ứng, nó có nghĩa là chủ sở hữu private key của tài khoản đã ký và do đó "ủy quyền" giao dịch và tài khoản sau đó được gọi là _người ký_. Việc tài khoản có phải là người ký hay không sẽ được thông báo với chương trình như một phần của siêu dữ liệu của tài khoản. Sau đó, các chương trình có thể sử dụng thông tin đó để đưa ra các quyết định có thẩm quyền.

## Chỉ-đọc

Các giao dịch có thể [chỉ ra](transactions.md#message-header-format) rằng một số tài khoản mà nó tham chiếu được coi là các _tài khoản chỉ-đọc_ để cho phép xử lý tài khoản song song giữa các giao dịch. Thời gian chạy cho phép các tài khoản chỉ-đọc được đọc đồng thời bởi nhiều chương trình. Nếu một chương trình cố gắng sửa đổi tài khoản chỉ-đọc, giao dịch sẽ bị từ chối trong thời gian chạy.

## Thực thi

Nếu một tài khoản được đánh dấu là "có thể thực thi" trong siêu dữ liệu của nó thì nó được coi là một chương trình có thể được thực thi bằng cách bao gồm public key của tài khoản đó một [id chương trình](transactions.md#program-id) của hướng dẫn. Tài khoản được đánh dấu là có thể thực thi trong quá trình triển khai chương trình thành công bởi trình tải sở hữu tài khoản.  Ví dụ: trong quá trình triển khai chương trình BPF, khi trình tải đã xác định rằng bytecode BPF trong dữ liệu của tài khoản là hợp lệ, trình tải sẽ đánh dấu vĩnh viễn tài khoản chương trình là có thể thực thi được.  Sau khi thực thi, thời gian chạy thực thi rằng dữ liệu của tài khoản (chương trình) là bất biến.

## Creating

Để tạo tài khoản, khách hàng tạo một _keypair_và đăng ký public key của nó bằng cách sử dụng hướng dẫn `SystemProgram::CreateAccount` với kích thước lưu trữ cố định được phân bổ trước tính bằng byte. Kích thước tối đa hiện tại của dữ liệu tài khoản là 10 megabyte.

Địa chỉ tài khoản có thể là bất kỳ giá trị 256 bit tùy ý nào và có các cơ chế để người dùng nâng cao tạo địa chỉ dẫn xuất (`SystemProgram::CreateAccountWithSeed`, [`Pubkey::CreateProgramAddress`](calling-between-programs.md#program-derived-addresses)).

Các tài khoản chưa từng được tạo qua chương trình hệ thống cũng có thể được chuyển cho các chương trình. Khi một lệnh tham chiếu đến một tài khoản chưa được tạo trước đó, chương trình sẽ được chuyển qua một tài khoản thuộc sở hữu của chương trình hệ thống, không có lamport và không có dữ liệu. Tuy nhiên, tài khoản sẽ phản ánh liệu nó có phải là người ký giao dịch hay không và do đó có thể được sử dụng như một cơ quan có thẩm quyền. Các cơ quan có thẩm quyền trong ngữ cảnh này chuyển đến chương trình rằng chủ sở hữu private key được liên kết với public key của tài khoản đã ký giao dịch. Public key của tài khoản có thể được chương trình biết đến hoặc được ghi lại trong một tài khoản khác và biểu thị một số loại quyền sở hữu hoặc quyền hạn đối với nội dung hoặc hoạt động mà chương trình kiểm soát hoặc thực hiện.

## Quyền sở hữu và chuyển nhượng cho các chương trình

Một tài khoản đã tạo được khởi tạo thành _sở hữu_ bởi một chương trình tích hợp có tên là Chương trình hệ thống và được gọi là _tài khoản hệ thống_. Tài khoản bao gồm siêu dữ liệu của "chủ sở hữu". Chủ sở hữu là một id chương trình. Thời gian chạy cấp cho chương trình quyền truy cập ghi vào tài khoản nếu id của nó khớp với chủ sở hữu. Đối với trường hợp của chương trình Hệ thống, thời gian chạy cho phép khách hàng chuyển các cổng và quan trọng là _chỉ định_ quyền sở hữu tài khoản, nghĩa là thay đổi chủ sở hữu thành id chương trình khác. Nếu một tài khoản không thuộc sở hữu của một chương trình, chương trình chỉ được phép đọc dữ liệu của nó và ghi có vào tài khoản.

## Thuê

Các tài khoản tồn tại trên Solana phải chịu một khoản phí lưu trữ được gọi là _tiền thuệ_ vì cụm phải tích cực duy trì dữ liệu để xử lý bất kỳ giao dịch nào trong tương lai trên đó. Điều này khác với Bitcoin và Ethereum, nơi lưu trữ tài khoản không phải chịu bất kỳ chi phí nào.

Tiền thuê được ghi nợ từ số dư của tài khoản trong thời gian chạy khi truy cập đầu tiên (bao gồm cả việc tạo tài khoản ban đầu) trong kỷ nguyên hiện tại bằng các giao dịch hoặc mỗi kỷ nguyên một lần nếu không có giao dịch. Phí hiện tại là một tỷ lệ cố định, được đo bằng byte-thời gian-kỷ nguyên. Phí có thể thay đổi trong tương lai.

Vì mục đích tính toán tiền thuê đơn giản, tiền thuê luôn được thu một lần, kỷ nguyên đầy đủ. Tiền thuê không được xếp hạng theo tỷ lệ, có nghĩa là không có phí cũng như không được hoàn lại tiền cho từng phần. Điều này có nghĩa là, khi tạo tài khoản, khoản tiền thuê đầu tiên được thu không phải cho một phần của kỷ nguyên hiện tại, mà được thu trước cho kỷ nguyên đầy đủ tiếp theo. Các bộ sưu tập tiền thuê tiếp theo dành cho các kỷ nguyên tiếp theo trong tương lai. Mặt khác, nếu số dư của một tài khoản đã được thu tiền thuê giảm xuống dưới mức phí thuê khác giữa kỷ nguyên, tài khoản sẽ tiếp tục tồn tại thông qua kỷ nguyên hiện tại và được thanh lọc ngay lập tức vào đầu kỷ nguyên sắp tới.

Các tài khoản có thể được miễn trả tiền thuê nếu duy trì số dư tối thiểu. Việc miễn-tiền thuê này được mô tả dưới đây.

### Tính tiền thuê

Lưu ý: Giá thuê có thể thay đổi trong tương lai.

Theo văn bản, phí thuê cố định là 19.055441478439427 lamport trên mỗi byte-epoch trên testnet và các cụm mainnet-beta. Một [kỷ nguyên](terminology.md#epoch) được dự định là 2 ngày (Đối với mạng devnet, phí thuê là 0.3608183131797095 lamport mỗi byte-epoch với kỷ nguyên 54m36s-long).

Giá trị này được tính theo mục tiêu 0.01 SOL mỗi mebibyte ngày (tương đương với 3.56 SOL mỗi mebibyte năm):

```text
Rent fee: 19.055441478439427 = 10_000_000 (0.01 SOL) * 365(approx. day in a year) / (1024 * 1024)(1 MiB) / (365.25/2)(epochs in 1 year)
```

Và tính toán tiền thuê được thực hiện với độ chính xác là `f64` và kết quả cuối cùng được rút gọn thành`u64` trong các lamport.

Việc tính toán tiền thuê bao gồm siêu dữ liệu tài khoản (địa chỉ, chủ sở hữu, các lamport, v. v.) trong kích thước của một tài khoản. Do đó, tài khoản nhỏ nhất có thể dùng để tính tiền thuê là 128 byte.

Ví dụ: một tài khoản được tạo với lần chuyển ban đầu là 10,000 lamport và không có dữ liệu bổ sung. Tiền thuê được ghi nợ ngay lập tức khi tạo ra, dẫn đến số dư là 7,561 lamport:


```text
Rent: 2,439 = 19.055441478439427 (rent rate) * 128 bytes (minimum account size) * 1 (epoch)
Account Balance: 7,561 = 10,000 (transfered lamports) - 2,439 (this account's rent fee for an epoch)
```

Số dư tài khoản sẽ giảm xuống còn 5,122 lamport vào thời điểm tiếp theo ngay cả khi không có hoạt động nào:

```text
Account Balance: 5,122 = 7,561 (current balance) - 2,439 (this account's rent fee for an epoch)
```

Theo đó, tài khoản có kích thước tối thiểu sẽ bị xóa ngay lập tức sau khi tạo nếu số lượng lamport được chuyển nhỏ hơn hoặc bằng 2,439.

### Miễn tiền thuê

Ngoài ra, một tài khoản có thể được thực hiện hoàn toàn miễn thu tiền thuê bằng cách ký gửi tiền thuê ít nhất 2 năm. Điều này được kiểm tra mỗi khi số dư tài khoản bị giảm và tiền thuê sẽ được ghi nợ ngay lập tức khi số dư xuống dưới số tiền tối thiểu.

Các tài khoản thực thi chương trình được yêu cầu trong thời gian chạy phải được miễn-tiền thuê để tránh bị thanh lọc.

Lưu ý: Sử dụng [`getMinimumBalanceForRentExemption`điểm cuối RPC](developing/clients/jsonrpc-api.md#getminimumbalanceforrentexemption) để tính số dư tối thiểu cho một kích thước tài khoản cụ thể. Tính toán sau đây chỉ mang tính chất minh họa.

Ví dụ: một chương trình thực thi có kích thước 15,000 byte yêu cầu số dư 105,290,880 lamport (= ~ 0.105 SOL) để được miễn tiền thuê:

```text
105,290,880 = 19.055441478439427 (fee rate) * (128 + 15_000)(account size including metadata) * ((365.25/2) * 2)(epochs in 2 years)
```
