---
title: Truyền khối Turbine
---

Một cụm Solana sử dụng cơ chế lan truyền khối nhiều lớp được gọi là _Turbine_ để phát các giao dịch tới tất cả các node với số lượng thông báo trùng lặp tối thiểu. Cụm phân chia chính nó thành các tập hợp nhỏ của các node, được gọi là _neighborhoods_. Mỗi node chịu trách nhiệm chia sẻ bất kỳ dữ liệu nào mà nó nhận được với các node khác trong vùng lân cận của nó, cũng như truyền tải dữ liệu tới một nhóm nhỏ các node trong vùng lân cận khác. Bằng cách này, mỗi node chỉ phải giao tiếp với một số lượng nhỏ các node.

Trong slot đó, leader node phân phối các shred giữa các validator node trong vùng lân cận đầu tiên \(layer 0\). Mỗi validator chia sẻ dữ liệu trong vùng lân cận của họ, nhưng cũng truyền lại các mẩu tin đó đến một node trong một số vùng lân cận trong layer tiếp theo \(layer 1\). Mỗi node layer-1 chia sẻ dữ liệu của chúng với các đồng nghiệp lân cận của chúng và truyền lại cho các node ở layer tiếp theo, v. v., cho đến khi tất cả các node trong cụm đã nhận được tất cả các mẩu tin nhỏ.

## Chuyển nhượng Vùng lân cận - Lựa chọn Trọng số

Để fanout mặt phẳng dữ liệu hoạt động, toàn bộ cụm phải thống nhất về cách phân chia cụm thành các vùng lân cận. Để đạt được điều này, tất cả các node validator được công nhận \(Các đồng nghiệp của TVU\) được sắp xếp theo tỷ lệ và được lưu trữ trong một danh sách. Danh sách này sau đó được lập chỉ mục theo các cách khác nhau để tìm ra ranh giới vùng lân cận và truyền lại các đồng nghiệp. Ví dụ, leader sẽ chỉ cần chọn các node đầu tiên để tạo thành layer 0. Đây sẽ tự động là những người có stake cao nhất, cho phép những phiếu bầu nặng nhất sẽ về với leader trước. Các node layer 0 và layer thấp hơn sử dụng cùng một logic để tìm các node lân cận và các node layer tiếp theo của chúng.

Để giảm khả năng bị tấn công vectơ, mỗi shred được truyền qua một cây ngẫu nhiên của các vùng lân cận. Mỗi node sử dụng cùng một tập hợp các node đại diện cho cụm. Một cây ngẫu nhiên được tạo từ tập hợp cho mỗi shred bằng cách sử dụng một hạt giống được lấy từ id leader, slot và chỉ mục shred.

## Cấu trúc Layer và Vùng lân cận

Leader hiện tại thực hiện các chương trình phát sóng ban đầu của nó tới hầu hết các node `DATA_PLANE_FANOUT`. Nếu layer 0 này nhỏ hơn số node trong cụm, thì cơ chế fanout mặt phẳng dữ liệu sẽ thêm các layer bên dưới. Các layer tiếp theo tuân theo các ràng buộc này để xác định dung lượng layer: Mỗi vùng lân cận chứa `DATA_PLANE_FANOUT` các node. Layer 0 bắt đầu với 1 vùng lân cận với các node fanout. Số lượng các node trong mỗi layer bổ sung tăng theo hệ số fanout.

Như đã đề cập ở trên, mỗi node trong một layer chỉ phải phát các shred của nó tới các vùng lân cận và đến chính xác 1 node trong một số vùng lân cận layer tiếp theo, thay vì cho mọi đồng đẳng TVU trong cụm. Một cách tốt để nghĩ về điều này là, layer 0 bắt đầu với 1 vùng lân cận với các node fanout, layer 1 thêm các fanout vùng lân cận, mỗi vùng có các node fanout và layer 2 sẽ có `fanout * number of nodes in layer 1`.

Bằng cách này, mỗi node chỉ phải giao tiếp với tối đa `2 * DATA_PLANE_FANOUT - 1` node.

Sơ đồ sau đây cho thấy cách Leader gửi các shred với tỉ lệ 2 đến Vùng lân cận 0 trong Layer 0 và cách các node trong Vùng lân cận 0 chia sẻ dữ liệu của chúng với nhau.

![Leader gửi các shred đến Vùng lân cận 0 trong Layer 0](/img/data-plane-seeding.svg)

Sơ đồ sau đây cho thấy cách người hâm mộ của Vùng lân cận 0 đến Vùng lân cận 1 và 2.

![Vùng lân cận 0 Fanout đến Vùng lân cận 1 và 2](/img/data-plane-fanout.svg)

Cuối cùng, sơ đồ sau đây hiển thị một cụm hai layer với fanout là 2.

![Cụm hai layer với Fanout là 2](/img/data-plane.svg)

### Giá trị Cấu hình

`DATA_PLANE_FANOUT` - Xác định kích thước của layer 0. Các layer tiếp theo phát triển theo hệ số `DATA_PLANE_FANOUT`. Số lượng node trong một vùng lân cận bằng giá trị fanout. Các vùng lân cận sẽ lấp đầy dung lượng trước khi thêm những vùng mới, tức là nếu một vùng lân cận không đầy _must_ nó phải là vùng cuối cùng.

Hiện tại, cấu hình sẽ được thiết lập khi khởi chạy cụm. Trong tương lai, các thông số này có thể được lưu trữ trên chuỗi, cho phép sửa đổi nhanh chóng khi kích thước cụm thay đổi.

## Tính toán tỷ lệ FEC bắt buộc

Turbine dựa vào việc truyền lại các gói giữa các validator. Do sự truyền lại, bất kỳ sự mất gói nào trên toàn mạng đều được cộng lại và xác suất gói không đến được đích của nó sẽ tăng lên trên mỗi bước nhảy. Tốc độ FEC cần phải tính đến việc mất gói trên toàn mạng và độ sâu lan truyền.

Nhóm được chia nhỏ là tập hợp các gói dữ liệu và mã hóa có thể được sử dụng để tái tái tạo lẫn nhau. Mỗi nhóm được chia nhỏ đều có khả năng bị lỗi, dựa trên khả năng xảy ra số lượng gói tin bị lỗi vượt quá tốc độ FEC. Nếu validator không thể tạo lại nhóm đã chia nhỏ, thì khối không thể được tạo lại và validator phải dựa vào sửa chữa để sửa chữa các khối.

Xác suất của nhóm shred không thành công có thể được tính bằng cách sử dụng phân phối nhị thức. Nếu tỷ lệ FEC là `16:4`, thì quy mô nhóm là 20 và ít nhất 4 trong số các shred phải không thành công để nhóm không thành công. Bằng tổng xác suất của 4 đường mòn trở lên không đạt trong tổng số 20 đường.

Xác suất khối thành công trong turbine:

- Probability of packet failure: `P = 1 - (1 - network_packet_loss_rate)^2`
- FEC rate: `K:M`
- Number of trials: `N = K + M`
- Shred group failure rate: `S = SUM of i=0 -> M for binomial(prob_failure = P, trials = N, failures = i)`
- Shreds per block: `G`
- Block success rate: `B = (1 - S) ^ (G / N)`
- Binomial distribution for exactly `i` results with probability of P in N trials is defined as `(N choose i) * P^i * (1 - P)^(N-i)`

Ví dụ:

- Tỷ lệ mất gói mạng là 15%.
- Mạng 50k tps tạo ra 6400 mảnh mỗi giây.
- Tỷ lệ FEC tăng tổng số shred trên mỗi khối theo tỷ lệ FEC.

Với tỷ lệ FEC: `16:4`

- `G = 8000`
- `P = 1 - 0.85 * 0.85 = 1 - 0.7225 = 0.2775`
- `S = SUM of i=0 -> 4 for binomial(prob_failure = 0.2775, trials = 20, failures = i) = 0.689414`
- `B = (1 - 0.689) ^ (8000 / 20) = 10^-203`

Với tỷ lệ FEC là `16:16`

- `G = 12800`
- `S = SUM of i=0 -> 32 for binomial(prob_failure = 0.2775, trials = 64, failures = i) = 0.002132`
- `B = (1 - 0.002132) ^ (12800 / 32) = 0.42583`

Với tỷ lệ FEC là `32:32`

- `G = 12800`
- `S = SUM of i=0 -> 32 for binomial(prob_failure = 0.2775, trials = 64, failures = i) = 0.000048`
- `B = (1 - 0.000048) ^ (12800 / 64) = 0.99045`

## Vùng lân cận

Sơ đồ sau đây cho thấy cách hai vùng lân cận trong các layer khác nhau tương tác. Để làm tê liệt một vùng lân cận, cần phải thất bại đủ các node \(erasure codes +1\) từ vùng lân cận ở trên. Vì mỗi vùng lân cận nhận được các shred từ nhiều node trong vùng lân cận ở layer trên, chúng tôi sẽ cần một sự cố mạng lớn ở các layer trên để kết thúc với dữ liệu không đầy đủ.

![Hoạt động bên trong của vùng lân cận](/img/data-plane-neighborhood.svg)
