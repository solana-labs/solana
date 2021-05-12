---
title: Đăng ký biểu quyết an toàn
---

## Đăng ký biểu quyết an toàn

Thiết kế này mô tả hành vi ký phiếu bổ sung sẽ làm cho quy trình an toàn hơn.

Hiện tại, Solana triển khai dịch vụ ký phiếu đánh giá từng phiếu bầu để đảm bảo không vi phạm điều kiện slashing. Dịch vụ có thể có các biến thể khác nhau, tùy thuộc vào khả năng của nền tảng phần cứng. Đặc biệt, nó có thể được sử dụng kết hợp với một vùng bảo mật \(chẳng hạn như SGX\). Enclave có thể tạo ra một khóa không đối xứng, để lộ một API code cho người dùng \(không đáng tin cậy\) để ký các giao dịch bỏ phiếu, trong khi vẫn giữ private key ký phiếu trong bộ nhớ được bảo vệ của nó.

Các phần sau đây phác thảo cách thức hoạt động của kiến ​​trúc này:

### Luồng tin nhắn

1. Node khởi tạo enclave khi khởi động

   - Enclave tạo ra một khóa không đối xứng và trả lại public key cho

     node

   - Keypair là phù du. Một keypair mới được tạo khi khởi động node. Một

     keypair mới cũng có thể được tạo trong thời gian chạy dựa trên một số được xác định

     tiêu chí.

   - Enclave trả về báo cáo chứng thực của nó cho node

2. Node thực hiện chứng thực vùng mã hóa \(ví dụ: sử dụng các API IAS của Intel\)

   - Node đảm bảo rằng Secure Enclave đang chạy trên TPM và

     được ký bởi một bên đáng tin cậy

3. Stakeholder của node cấp quyền cho khóa tạm thời để sử dụng stake của nó.

   Quá trình này phải được xác định.

4. Phần mềm không đáng tin cậy, không được mã hóa của node gọi phần mềm mã hóa đáng tin cậy

   sử dụng giao diện của nó để ký các giao dịch và dữ liệu khác.

   - Trong trường hợp ký biểu quyết, node cần xác minh PoH. PoH

     xác minh là một phần không thể thiếu của việc ký kết. Vùng đất sẽ

     được trình bày với một số dữ liệu có thể xác minh được để kiểm tra trước khi ký vào phiếu bầu.

   - Quá trình tạo dữ liệu có thể xác minh trong không gian không đáng tin cậy phải được xác định

### Xác minh PoH

1. Khi node bỏ phiếu cho một mục nhập `X`, sẽ có một khoảng lockout `N`, đối với

   mà nó không thể bỏ phiếu cho một fork không chứa `X` trong lịch sử của nó.

2. Mỗi khi node bỏ phiếu trên dẫn xuất của `X`,, hãy nói `X+y`, lockout

   khoảng thời gian cho `X`> tăng một hệ số `F` \(tức là node thời lượng không thể bỏ phiếu

   một fork không chứa `X` tăng\).

   - Thời hạn lockout cho `X+y` vẫn là `N` cho đến khi node bỏ phiếu lại.

3. Thời hạn lockout gia tăng được giới hạn \(ví dụ: hệ số `F` áp dụng tối đa 32

   lần \).

4. Vùng ký tên không được ký vào một biểu quyết vi phạm chính sách này. Điều này

   có nghĩa

   - Enclave được khởi tạo bằng `N`, `F` và `Factor cap`
   - Enclave lưu trữ `Factor cap` số lượng ID mục nhập mà trên đó node

     đã bình chọn trước đó

   - Yêu cầu ký tên chứa ID mục nhập cho phiếu bầu mới
   - Enclave xác minh rằng ID mục nhập của phiếu bầu mới nằm trên đúng fork

     \(tuân theo các quy tắc \# 1 và \# 2 ở trên\(

### Xác minh tổ tiên

Đây là cách tiếp cận thay thế, mặc dù, ít chắc chắn hơn để xác minh fork bỏ phiếu. 1. Validator duy trì một tập hợp các node đang hoạt động trong cụm 2. Nó quan sát các phiếu bầu từ tập hợp đang hoạt động trong giai đoạn biểu quyết cuối cùng 3. Nó lưu trữ tổ tiên/last_tick mà tại đó mỗi node đã bỏ phiếu 4. Nó gửi yêu cầu bỏ phiếu mới đến dịch vụ ký phiếu bầu

- Nó bao gồm các phiếu bầu trước đó từ các node trong tập hợp đang hoạt động và

  tổ tiên tương ứng

  1. Người ký kiểm tra xem các phiếu bầu trước đó có chứa phiếu bầu từ validator hay không,

     và tổ tiên phiếu bầu phù hợp với phần lớn các node

- Nó ký vào phiếu bầu mới nếu kiểm tra thành công
- Nó khẳng định \(phát ra một cảnh báo nào đó\) nếu việc kiểm tra không thành công

Tiền đề là validator có thể bị giả mạo nhiều nhất một lần để bỏ phiếu cho dữ liệu không chính xác. Nếu ai đó chiếm đoạt validator và gửi yêu cầu bỏ phiếu cho dữ liệu không có thật, phiếu bầu đó sẽ không được đưa vào PoH \(vì nó sẽ bị cụm từ chối\). Lần tiếp theo validator gửi yêu cầu ký vào phiếu bầu, dịch vụ ký sẽ phát hiện phiếu bầu cuối cùng của validator bị thiếu \(như một phần của

## 5 ở trên\).

### Xác định fork

Do thực tế là enclave không thể xử lý PoH, nó không có kiến thức trực tiếp về lịch sử fork của một cuộc bỏ phiếu xác nhận đã gửi. Mỗi enclave phải được bắt đầu bằng _bộ hoạt động_ hiện tại của các public key. Một validator phải gửi phiếu bầu hiện tại của mình cùng với phiếu bầu của bộ đang hoạt động \(bao gồm cả chính nó\) mà nó đã quan sát được trong slot của phiếu bầu trước đó. Bằng cách này, enclave có thể phỏng đoán các phiếu bầu đi kèm với phiếu bầu trước đó của validator và do đó fork được bỏ phiếu. Điều này không thể xảy ra đối với phiếu bầu đã gửi ban đầu của validator, vì nó sẽ không có một slot 'trước đó' để tham chiếu. Để giải thích điều này, việc đóng băng biểu quyết ngắn sẽ được áp dụng cho đến khi biểu quyết thứ hai được gửi có chứa các phiếu bầu trong bộ đang hoạt động, cùng với phiếu bầu của chính nó, tại đỉnh cao của cuộc bỏ phiếu ban đầu.

### Cấu hình Enclave

Một khách hàng staking phải có thể định cấu hình để ngăn việc bỏ phiếu trên các fork không hoạt động. Cơ chế này nên sử dụng bộ hoạt động đã biết của khách hàng `N_active` cùng với ngưỡng bỏ phiếu `N_vote` và độ sâu ngưỡng `N_depth` để xác định xem có nên tiếp tục bỏ phiếu trên một fork đã gửi hay không. Cấu hình này phải có dạng quy tắc sao cho khách hàng sẽ chỉ bỏ phiếu trên một fork nếu nó quan sát nhiều hơn `N_vote` tại `N_depth`. Trên thực tế, điều này đại diện cho khách hàng xác nhận rằng họ đã quan sát thấy một số xác suất về tính kinh tế cuối cùng của đợt fork được gửi ở độ sâu mà một cuộc bỏ phiếu bổ sung sẽ tạo ra một lockout trong một khoảng thời gian không mong muốn nếu đợt fork đó không hoạt động.

### Những thách thức

1. Tạo dữ liệu có thể xác minh trong không gian không đáng tin cậy để xác minh PoH trong

   bao vây.

2. Cần cơ sở hạ tầng để cấp stake cho một khóa tạm thời.
