---
title: Xác nhận Lạc quan và Slashing
---

Tiến trình xác nhận lạc quan có thể được theo dõi tại đây

https://github.com/solana-labs/solana/projects/52

Vào cuối tháng 5, mainnet-beta sẽ chuyển sang 1.1 và testnet chuyển sang 1.2. Với 1.2, testnet sẽ hoạt động như thể nó có tính cuối cùng lạc quan miễn là ít nhất không quá 4,66% các validator đang hoạt động có hại. Các ứng dụng có thể cho rằng 2/3 số phiếu bầu được quan sát trong gossip xác nhận một khối hoặc ít nhất 4,66% mạng đang vi phạm giao thức.

## Làm thế nào để nó hoạt động?

Ý tưởng chung là các validator phải tiếp tục bỏ phiếu sau đợt fork cuối cùng của họ, trừ khi validator có thể tạo ra bằng chứng rằng đợt fork hiện tại của họ có thể không đạt được mục đích cuối cùng. Cách các validator xây dựng bằng chứng này là thu thập phiếu bầu cho tất cả các fork ngoại trừ các fork của chúng. Nếu tập hợp các phiếu bầu hợp lệ đại diện cho hơn 1/3 + X trọng lượng stake của kỷ nguyên, có thể không phải là cách để fork hiện tại của các validator đạt đến 2/3+ cuối cùng. Validator băm bằng chứng (tạo nhân chứng) và gửi nó cùng với phiếu bầu của họ cho fork thay thế. Nhưng nếu 2/3+ phiếu bầu cho cùng một khối, thì không thể có bất kỳ validator nào tạo ra bằng chứng này và do đó không validator nào có thể chuyển đổi các fork và khối này cuối cùng sẽ được hoàn thiện.

## Đánh đổi

Biên độ an toàn là 1/3+X, trong đó X đại diện cho số lượng stake tối thiểu sẽ bị cắt giảm trong trường hợp giao thức bị vi phạm. Sự cân bằng là khả năng hoạt động hiện đã giảm 2 lần trong trường hợp xấu nhất. Nếu nhiều hơn 1/3 - 2X mạng không khả dụng, mạng có thể bị đình trệ và sẽ chỉ tiếp tục hoàn thiện các khối sau khi mạng khôi phục dưới 1/3 - 2X của các node bị lỗi. Cho đến nay, chúng tôi đã không quan sát thấy sự thiếu khả dụng lớn trên mạng chính, cosmos hoặc tezos của chúng tôi. Đối với mạng của chúng tôi, chủ yếu bao gồm các hệ thống có tính sẵn sàng cao, điều này có vẻ khó xảy ra. Hiện tại, chúng tôi đã đặt tỷ lệ phần trăm ngưỡng là 4,66%, có nghĩa là nếu 23,68% không thành công, mạng có thể ngừng hoàn thiện các khối. Đối với mạng của chúng tôi, chủ yếu bao gồm các hệ thống có tính sẵn sàng cao, mức độ sẵn có giảm 23,68% dường như không liên kết. 1:10^12 odds assuming five 4.7% staked nodes with 0.995 of uptime.

## Bảo mật

Số phiếu bầu trung bình dài hạn cho mỗi slot là 670.000.000 phiếu bầu / 12.000.000 slot, hoặc 55 trong số 64 phiếu bầu validator. Điều này bao gồm các khối bị bỏ lỡ do lỗi của nhà sản xuất khối. Khi khách hàng thấy 55/64 hoặc ~ 86% xác nhận một khối, nó có thể mong đợi rằng ~ 24% hoặc `(86 - 66.666.. + 4.666..)%` mạng phải bị cắt để khối này không thể hoàn thành toàn bộ.

## Tại sao lại là Solana ?

Cách tiếp cận này có thể được xây dựng trên các mạng khác, nhưng độ phức tạp khi triển khai được giảm đáng kể trên Solana vì phiếu bầu của chúng tôi có thời gian chờ dựa trên VDF có thể cho phép. Không rõ liệu các bằng chứng chuyển đổi có thể dễ dàng được xây dựng trong các mạng có giả định yếu về thời gian hay không.

## Lộ trình Slashing

Slashing là một vấn đề khó và càng khó hơn khi mục tiêu của mạng là có độ trễ thấp nhất có thể. Sự cân bằng là đặc biệt rõ ràng khi tối ưu hóa độ trễ. Ví dụ, lý tưởng nhất là các validator nên bỏ phiếu và tuyên truyền phiếu bầu của họ trước khi bộ nhớ được đồng bộ hóa với ổ đĩa cứng, có nghĩa là nguy cơ tham nhũng trạng thái cục bộ cao hơn nhiều.

Về cơ bản, mục tiêu slashing của chúng tôi là cắt giảm 100% trong các trường hợp node cố ý vi phạm các quy tắc an toàn và 0% trong quá trình hoạt động thông thường. Cách chúng tôi hướng tới để đạt được điều đó trước tiên là thực hiện slashing qua các bằng chứng mà không phải là bất kỳ phương pháp slashing tự động nào.

Ngay bây giờ, để có sự đồng thuận thường xuyên, sau khi vi phạm an toàn, mạng sẽ tạm dừng. Chúng tôi có thể phân tích dữ liệu và tìm ra ai chịu trách nhiệm và đề xuất rằng stake nên được cắt giảm sau khi khởi động lại. Một phương pháp tiếp cận tương tự sẽ được sử dụng với một sự tự tin lạc quan. Có thể dễ dàng quan sát thấy một vi phạm an toàn lạc quan, nhưng trong những trường hợp bình thường, một vi phạm an toàn xác nhận lạc quan có thể không ngăn mạng. Khi vi phạm đã được quan sát thấy, các validator sẽ đóng băng tiền stake bị ảnh hưởng trong kỷ nguyên tiếp theo và sẽ quyết định về việc nâng cấp tiếp theo nếu vi phạm yêu cầu slashing.

Về lâu dài, các giao dịch sẽ có thể thu hồi một phần tài sản thế chấp cắt giảm nếu vi phạm an toàn lạc quan được chứng minh. Trong trường hợp đó, mỗi khối được bảo hiểm bởi mạng.
