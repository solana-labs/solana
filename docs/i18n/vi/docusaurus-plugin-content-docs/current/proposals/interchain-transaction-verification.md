---
title: Xác minh giao dịch liên chuỗi
---

## Sự cố

Các ứng dụng liên chuỗi không phải là mới đối với hệ sinh thái tài sản kỹ thuật số; trên thực tế, ngay cả các sàn giao dịch tập trung nhỏ hơn vẫn còn hạn chế tất cả các ứng dụng chuỗi đơn được tập hợp lại của người dùng và khối lượng. Họ đưa ra mức định giá lớn và đã tốn nhiều năm để cải thiện các sản phẩm cốt lõi của mình cho một loạt những người dùng cuối cùng. Tuy nhiên, các hoạt động cơ bản của chúng tập trung vào các cơ chế yêu cầu người dùng đơn phương tin tưởng chúng, thường ít hoặc không đòi lại hoặc bảo vệ khỏi mất mát ngẫu nhiên. Điều này đã dẫn đến hệ sinh thái tài sản kỹ thuật số rộng lớn hơn bị phá vỡ dọc theo các đường mạng bởi vì các giải pháp khả năng tương tác thường:

- Có kỹ thuật phức tạp để thực hiện đầy đủ
- Tạo cấu trúc khuyến khích quy mô mạng không ổn định
- Yêu cầu sự hợp tác nhất quán và ở mức độ cao giữa các bên liên quan

## Giải pháp đề xuất

Xác minh thanh toán đơn giản \(SPV\) là một thuật ngữ chung cho một loạt các phương pháp luận khác nhau được sử dụng bởi các khách hàng nhẹ trên hầu hết các mạng blockchain chính để xác minh các khía cạnh của trạng thái mạng mà không có gánh nặng lưu trữ và duy trì đầy đủ chuỗi chính. Trong hầu hết các trường hợp, điều này có nghĩa là dựa vào một dạng cây băm để cung cấp bằng chứng về sự hiện diện của một giao dịch nhất định trong một khối nhất định bằng cách so sánh với hàm băm gốc trong tiêu đề của khối đó hoặc tương đương. Điều này cho phép một khách hàng nhẹ hoặc ví tiền tự đạt đến mức độ chắc chắn về các sự kiện trên chuỗi với mức độ tin cậy tối thiểu cần thiết đối với các node mạng.

Theo truyền thống, quá trình tập hợp và xác thực các bằng chứng này được thực hiện ngoài chuỗi bởi các node, ví hoặc các khách hàng khác, nhưng nó cũng cung cấp một cơ chế tiềm năng để xác minh trạng thái liên chuỗi. Tuy nhiên, bằng cách chuyển khả năng xác thực các bằng chứng SPV trên chuỗi như một hợp đồng thông minh trong khi tận dụng các thuộc tính lưu trữ vốn có của blockchain, có thể xây dựng một hệ thống để phát hiện và xác minh theo chương trình các giao dịch trên các mạng khác mà không cần sự tham gia của bất kỳ loại tiên tri đáng tin cậy hoặc cơ chế đồng thuận nhiều giai đoạn phức tạp. Khái niệm này có thể áp dụng chung cho bất kỳ mạng nào có cơ chế SPV và thậm chí có thể được vận hành song phương trên các nền tảng hợp đồng thông minh khác, mở ra khả năng chuyển giao giá trị liên chuỗi giá rẻ, nhanh chóng mà không cần dựa vào tài sản thế chấp, mã băm, hoặc trung gian đáng tin cậy.

Việc chọn tận dụng các cơ chế ổn định và được thiết lập tốt đã phổ biến cho tất cả các blockchain chính cho phép các giải pháp tương tác dựa trên SPV trở nên đơn giản hơn đáng kể so với các phương pháp tiếp cận nhiều giai đoạn được sắp xếp. Là một phần của điều này, họ phân phối với nhu cầu về các tiêu chuẩn giao tiếp chuỗi chéo được thống nhất rộng rãi và các tổ chức đa bên lớn viết chúng, vì một loạt các dịch vụ riêng lẻ có thể dễ dàng được sử dụng bởi các cuộc gọi bằng một dạng trừu tượng phổ biến. Điều này sẽ đặt nền tảng cho một loạt các ứng dụng và hợp đồng có thể tương tác với nhau trên mọi hệ sinh thái nền tảng đa dạng và đang phát triển.

## Thuật ngữ

SPV Program - Giao diện hướng tới khách hàng cho hệ thống SPV liên chuỗi, quản lý các vai trò của người tham gia. SPV Engine - Xác thực các bằng chứng giao dịch, tập hợp con của Chương trình SPV. Client - Người gọi đến Chương trình SPV, thường là một hợp đồng solana khác. Prover - Bên tạo bằng chứng cho các giao dịch và gửi chúng cho chương trình SPV. Transaction Proof - Được tạo bởi Provers, chứa tham chiếu bằng chứng merkle, giao dịch và blockheader. Merkle Proof - Bằng chứng SPV cơ bản xác nhận sự hiện diện của một giao dịch trong một khối nhất định. Block Header - Đại diện cho các tham số cơ bản và vị trí tương đối của một khối nhất định. Proof Request - Một đơn đặt hàng do khách hàng đặt để xác minh giao dịch \(s\) của người giao dịch. Header Store - Cấu trúc dữ liệu để lưu trữ và tham chiếu đến phạm vi tiêu đề khối trong các bằng chứng. Client Request - Giao dịch từ khách hàng đến Chương trình SPV để kích hoạt tạo Yêu cầu Bằng chứng. Sub-account - Một tài khoản Solana thuộc sở hữu của một tài khoản hợp đồng khác, không có private key.

## Dịch vụ

Các Chương trình SPV chạy dưới dạng các hợp đồng được triển khai trên mạng Solana và duy trì một loại thị trường công khai cho các bằng chứng SPV cho phép bất kỳ bên nào gửi cả hai yêu cầu về bằng chứng cũng như bản thân các bằng chứng để xác minh theo yêu cầu. Sẽ có nhiều phiên bản Chương trình SPV hoạt động tại bất kỳ thời điểm nào, ít nhất một phiên bản cho mỗi mạng bên ngoài được kết nối và có thể nhiều phiên bản cho mỗi mạng. Các phiên bản chương trình SPV sẽ tương đối nhất quán trong các tập hợp tính năng và API cấp cao của chúng với một số biến thể giữa các nền tảng tiền tệ \(Bitcoin, Litecoin\) và các nền tảng hợp đồng thông minh do tiềm năng xác minh các thay đổi trạng thái mạng không chỉ đơn giản là các giao dịch. Trong mọi trường hợp bất kể mạng nào, Chương trình SPV dựa vào một thành phần bên trong được gọi là công cụ SPV để cung cấp xác minh không trạng thái về các bằng chứng SPV thực tế mà ứng dụng khách cấp cao hơn phải đối mặt với các tính năng và api được xây dựng. Công cụ SPV yêu cầu triển khai mạng cụ thể, nhưng cho phép dễ dàng mở rộng hệ sinh thái liên chuỗi lớn hơn bởi bất kỳ nhóm nào chọn thực hiện việc triển khai đó và đưa nó vào chương trình SPV tiêu chuẩn để triển khai.

Đối với các mục đích của các Yêu cầu Bằng chứng, người yêu cầu được gọi là khách hàng của chương trình, trong hầu hết các trường hợp, nếu không phải tất cả các trường hợp sẽ là một Hợp đồng Solana khác. Khách hàng có thể chọn gửi một yêu cầu liên quan đến một giao dịch cụ thể hoặc bao gồm một bộ lọc rộng hơn có thể áp dụng cho bất kỳ phạm vi thông số nào của giao dịch bao gồm đầu vào, đầu ra và số tiền của nó. Ví dụ, khách hàng có thể gửi yêu cầu cho bất kỳ giao dịch nào được gửi từ một địa chỉ nhất định A đến địa chỉ B với số tiền X sau một thời gian nhất định. Cấu trúc này có thể được sử dụng trong một loạt các ứng dụng, chẳng hạn như xác minh khoản thanh toán dự định cụ thể trong trường hợp hoán đổi nguyên tử hoặc phát hiện sự di chuyển của tài sản đảm bảo cho một khoản vay.

Sau khi gửi Yêu cầu của khách hàng, giả sử rằng nó đã được xác thực thành công, chương trình SPV sẽ tạo một tài khoản yêu cầu bằng chứng để theo dõi tiến trình của yêu cầu. Các prover sử dụng tài khoản để chỉ định yêu cầu mà họ dự định điền vào các bằng chứng mà họ gửi để xác thực, tại thời điểm đó, chương trình SPV xác nhận các bằng chứng đó và nếu thành công, sẽ lưu chúng vào dữ liệu tài khoản của tài khoản yêu cầu. Khách hàng có thể theo dõi trạng thái yêu cầu của họ và xem bất kỳ giao dịch áp dụng nào cùng với các bằng chứng của họ bằng cách truy vấn dữ liệu tài khoản của tài khoản yêu cầu. Trong các lần lặp lại trong tương lai khi được Solana hỗ trợ, quy trình này sẽ được đơn giản hóa bằng các hợp đồng xuất bản các sự kiện thay vì yêu cầu quy trình kiểu thăm dò ý kiến ​​như đã mô tả.

## Thực hiện

Cơ chế SPV liên chuỗi Solana bao gồm các thành phần và thành phần tham gia sau:

### Động cơ SPV

Một hợp đồng được triển khai trên Solana để xác minh các bằng chứng SPV cho người gọi một cách vô trạng thái. Nó có vai trò là đối số để xác nhận:

- Một bằng chứng SPV ở định dạng chính xác của blockchain được liên kết với chương trình
- Tham chiếu \(các\) tiêu đề khối liên quan để so sánh bằng chứng đó với
- Các thông số cần thiết của giao dịch để xác minh

  Nếu bằng chứng được đề cập được xác thực thành công, chương trình SPV sẽ lưu bằng chứng

  xác minh đó vào tài khoản yêu cầu, tài khoản này có thể được người gọi lưu vào

  dữ liệu tài khoản của nó hoặc được xử lý theo cách khác khi cần thiết. Các chương trình SPV cũng phơi bày

  các tiện ích và cấu trúc được sử dụng để trình bày và xác nhận tiêu đề,

  các giao dịch, các hàm băm, v. v. trên cơ sở từng chuỗi.

### Chương trình SPV

Một hợp đồng được triển khai trên Solana, điều phối và làm trung gian tương tác giữa các Khách hàng và các prover và quản lý việc xác thực các yêu cầu, các tiêu đề, các bằng chứng, v. v. Đây là điểm truy cập chính để các hợp đồng của Khách hàng truy cập vào liên chuỗi. Cơ chế SPV. Nó cung cấp các tính năng cốt lõi sau:

- Gửi yêu cầu bằng chứng - cho phép khách hàng yêu cầu một bằng chứng cụ thể hoặc một tập hợp bằng chứng
- Hủy yêu cầu bằng chứng - cho phép khách hàng hủy bỏ một yêu cầu đang chờ xử lý
- Điền yêu cầu bằng chứng - được sử dụng bởi các Prover để gửi xác thực một bằng chứng tương ứng với một Yêu cầu Bằng chứng nhất định

  Chương trình SPV duy trì một danh sách có sẵn công khai về Bằng chứng hợp lệ đang chờ xử lý

  Yêu cầu trong dữ liệu tài khoản của nó vì lợi ích của các Prover, người giám sát nó và

  gửi kèm các tham chiếu đến các yêu cầu mục tiêu với các bằng chứng đã gửi của họ.

### Yêu cầu Bằng chứng

Một tin nhắn do Khách hàng gửi đến công cụ SPV biểu thị yêu cầu bằng chứng về một giao dịch cụ thể hoặc một tập hợp các giao dịch. Yêu cầu Bằng chứng có thể chỉ định thủ công một giao dịch nhất định bằng hàm băm của nó hoặc có thể chọn gửi một bộ lọc phù hợp với nhiều giao dịch hoặc nhiều loại giao dịch. Ví dụ, một bộ lọc đối sánh “bất kỳ giao dịch nào từ địa chỉ xxx đến địa chỉ yyy” có thể được sử dụng để phát hiện việc thanh toán một khoản nợ hoặc thanh toán một giao dịch hoán đổi liên chuỗi. Tương tự như vậy, một bộ lọc khớp với “bất kỳ giao dịch nào từ địa chỉ xxx” có thể được sử dụng bởi một hợp đồng cho vay hoặc đúc mã thông báo tổng hợp để theo dõi và phản ứng với những thay đổi trong thế chấp. Yêu cầu Bằng chứng được gửi với một khoản phí, được hợp đồng công cụ SPV giải ngân cho Người cung cấp phù hợp sau khi một bằng chứng khớp với yêu cầu đó được xác thực.

### Yêu cầu Book

Danh sách công khai các Yêu cầu Bằng chứng mở, hợp lệ có sẵn cho các nhà cung cấp dịch vụ để điền hoặc khách hàng có thể hủy bỏ. Gần giống với sổ đặt hàng trong một sàn giao dịch, nhưng với một loại danh sách duy nhất chứ không phải là hai mặt riêng biệt. Nó được lưu trữ trong dữ liệu tài khoản của chương trình SPV.

### Bằng chứng

Một bằng chứng về sự hiện diện của một giao dịch nhất định trong blockchain được đề cập. Các bằng chứng bao gồm cả bằng chứng merkle thực tế và \(các\) tham chiếu đến một chuỗi các tiêu đề khối tuần tự hợp lệ. Chúng được các Prover xây dựng và đệ trình theo các thông số kỹ thuật của Yêu cầu Bằng chứng có sẵn công khai được chương trình SPV lưu trữ trên sổ yêu cầu. Sau khi Xác thực, chúng được lưu vào dữ liệu tài khoản của Yêu cầu Bằng chứng liên quan, Khách hàng có thể sử dụng dữ liệu này để theo dõi trạng thái của yêu cầu.

### Khách hàng

Người khởi tạo yêu cầu một bằng chứng giao dịch. Khách hàng thường sẽ là các hợp đồng khác như một phần của ứng dụng hoặc các sản phẩm tài chính cụ thể như cho vay, hoán đổi, ký quỹ, v. v. Khách hàng trong bất kỳ chu kỳ quy trình xác minh nhất định nào ban đầu sẽ gửi một Yêu cầu khách hàng thông báo các thông số và phí và nếu được xác thực thành công, chương trình SPV sẽ tạo ra một tài khoản Yêu cầu Bằng chứng. Khách hàng cũng có thể gửi Yêu cầu hủy tham chiếu đến Yêu cầu bằng chứng đang hoạt động để biểu thị nó là không hợp lệ cho mục đích gửi bằng chứng.

### Prover

Người gửi một bằng chứng điền vào Yêu cầu Bằng chứng. Các Prover theo dõi sổ yêu cầu của chương trình SPV để biết các Yêu cầu Bằng chứng còn tồn đọng và tạo các bằng chứng phù hợp, họ gửi cho chương trình SPV để xác nhận. Nếu bằng chứng được chấp nhận, khoản phí liên quan đến Yêu cầu Bằng chứng được đề cập sẽ được giải ngân cho Prover. Các Prover thường hoạt động như các node Blockstreamer Solana cũng có quyền truy cập vào một node Bitcoin, chúng sử dụng cho mục đích xây dựng bằng chứng và truy cập tiêu đề khối.

### Lưu trữ tiêu đề

Một cấu trúc dữ liệu dựa trên tài khoản được sử dụng để duy trì tiêu đề khối nhằm mục đích đưa vào các bằng chứng đã gửi bằng cách tham chiếu đến tài khoản lưu trữ tiêu đề. lưu trữ tiêu đề có thể được duy trì bởi các thực thể độc lập, vì xác thực chuỗi tiêu đề là một thành phần của cơ chế xác thực bằng chứng chương trình SPV. Các khoản phí được thanh toán bằng Yêu cầu bằng chứng cho Người chứng minh được phân chia giữa người gửi bằng chứng merkle và lưu trữ tiêu đề được tham chiếu trong bằng chứng đã gửi. Do hiện tại không thể phát triển dung lượng dữ liệu tài khoản đã được phân bổ, trường hợp sử dụng yêu cầu cấu trúc dữ liệu có thể phát triển vô thời hạn mà không cần tái cân bằng. Tài khoản phụ là tài khoản thuộc sở hữu của chương trình SPV mà không có các private key riêng được sử dụng để lưu trữ bằng cách phân bổ các blockheader cho dữ liệu tài khoản của họ. Nhiều cách tiếp cận tiềm năng để triển khai hệ thống lưu trữ tiêu đề là khả thi:

Lưu trữ Tiêu đề trong các tài khoản phụ của chương trình được lập chỉ mục theo địa chỉ Công khai:

- Mỗi tài khoản phụ có một tiêu đề và có public key khớp với blockhash
- Yêu cầu số lượng tra cứu dữ liệu tài khoản giống như xác nhận cho mỗi lần xác minh
- Giới hạn về số lượng xác nhận \(15-20\) thông qua mức trần dữ liệu giao dịch tối đa
- Không có sự trùng lặp trên toàn mạng của các tiêu đề riêng lẻ

Danh sách được liên kết gồm nhiều tài khoản-phụ lưu trữ các tiêu đề:

- Duy trì chỉ mục tuần tự của các tài khoản lưu trữ, nhiều tiêu đề trên mỗi tài khoản lưu trữ
- Tối đa 2 tra cứu dữ liệu tài khoản cho &gt;99.9% số lần xác minh \(1 cho hầu hết\)
- Định dạng địa chỉ dữ liệu tuần tự nhỏ gọn cho phép bất kỳ số lượng xác nhận và tra cứu nhanh
- Tạo điều kiện thuận lợi cho việc sao chép tiêu đề trên toàn mạng không hiệu quả
