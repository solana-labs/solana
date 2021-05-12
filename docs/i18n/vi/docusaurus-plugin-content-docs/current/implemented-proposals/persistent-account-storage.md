---
title: Lưu trữ tài khoản liên tục
---

## Lưu trữ tài khoản liên tục

Tập hợp các Tài khoản đại diện cho trạng thái tính toán hiện tại của tất cả các giao dịch đã được xử lý bởi validator. Mỗi validator cần duy trì toàn bộ tập hợp này. Mỗi khối được mạng đề xuất đại diện cho một thay đổi đối với tập hợp này và vì mỗi khối là một điểm khôi phục tiềm năng nên các thay đổi cần phải được hoàn nguyên.

Lưu trữ liên tục như NVME rẻ hơn từ 20 đến 40 lần so với DDR. Vấn đề với lưu trữ liên tục là hiệu suất ghi và đọc chậm hơn nhiều so với DDR và ​​cần phải cẩn thận trong cách dữ liệu được đọc hoặc ghi vào. Cả hai lần đọc và ghi đều có thể được phân chia giữa nhiều ổ lưu trữ và được truy cập song song. Thiết kế này đề xuất một cấu trúc dữ liệu cho phép đọc đồng thời và ghi đồng thời bộ nhớ. Các văn bản được tối ưu hóa bằng cách sử dụng cấu trúc dữ liệu AppendVec, cho phép một người viết thêm vào trong khi vẫn cho phép truy cập vào nhiều trình đọc đồng thời. Chỉ mục tài khoản duy trì một con trỏ đến vị trí nơi tài khoản được nối vào mỗi lần fork, do đó loại bỏ nhu cầu kiểm tra trạng thái rõ ràng.

## AppendVec

AppendVec là một cấu trúc dữ liệu cho phép đọc ngẫu nhiên đồng thời với một trình ghi chỉ-phụ thêm. Việc tăng hoặc thay đổi kích thước dung lượng của AppendVec yêu cầu quyền truy cập độc quyền. Điều này được thực hiện với một nguyên tử `offset`, được cập nhật ở cuối một phụ lục hoàn chỉnh.

Bộ nhớ cơ bản cho AppendVec là một tệp được ánh xạ bộ nhớ. Các tệp được ánh xạ bộ nhớ cho phép truy cập ngẫu nhiên nhanh chóng và phân trang được xử lý bởi Hệ điều hành.

## Chỉ mục tài khoản

Chỉ mục tài khoản được thiết kế để hỗ trợ một chỉ mục duy nhất cho tất cả các Tài khoản hiện được fork.

```text
type AppendVecId = usize;

type Fork = u64;

struct AccountMap(Hashmap<Fork, (AppendVecId, u64)>);

type AccountIndex = HashMap<Pubkey, AccountMap>;
```

Chỉ mục là bản đồ của tài khoản Pubkey đến bản đồ của các Fork và vị trí của dữ liệu Tài khoản trong AppendVec. Để tải phiên bản tài khoản cho một Fork cụ thể:

```text
/// Load the account for the pubkey.
/// This function will load the account from the specified fork, falling back to the fork's parents
/// * fork - a virtual Accounts instance, keyed by Fork.  Accounts keep track of their parents with Forks,
///       the persistent store
/// * pubkey - The Account's public key.
pub fn load_slow(&self, id: Fork, pubkey: &Pubkey) -> Option<&Account>
```

Việc đọc được thỏa mãn bằng cách trỏ đến một vị trí được ánh xạ bộ nhớ trong phần `AppendVecId` bù được lưu trữ. Ham chiếu có thể được trả lại mà không cần bản sao.

### Các fork gốc

[Tower BFT](tower-bft.md) cuối cùng chọn một fork làm fork gốc và fork bị bóp méo. Một fork gốc/bẹp dúm không thể được cuộn lại.

Khi fork bị bóp nghẹt, tất cả các tài khoản cha mẹ của nó chưa có mặt trong fork sẽ được đưa vào fork bằng cách cập nhật các chỉ mục. Các tài khoản có số dư bằng 0 trong đợt fork bị bóp nghẹt sẽ bị xóa khỏi đợt fork bằng cách cập nhật các chỉ mục.

Một tài khoản có thể được _garbage-collected_ khi việc thu thập tài khoản khiến nó không thể truy cập được.

Có ba tùy chọn khả thi:

- Duy trì một HashSet gồm các fork gốc. Một cái dự kiến sẽ được tạo ra mỗi giây. Toàn bộ cây có thể được thu gom sau đó. Ngoài ra, nếu mọi fork đều giữ một số lượng tài khoản tham chiếu, thì việc thu thập rác có thể xảy ra bất cứ khi nào một vị trí chỉ mục được cập nhật.
- Loại bỏ bất kỳ fork bị cắt bớt nào khỏi chỉ mục. Bất kỳ fork nào còn lại có số lượng thấp hơn số gốc đều có thể được coi là gốc.
- Quét chỉ mục, di chuyển bất kỳ gốc cũ nào sang gốc mới. Bất kỳ fork nào còn lại thấp hơn gốc mới có thể bị xóa sau đó.

## Append-only Writes

Tất cả các cập nhật cho Tài khoản xảy ra dưới dạng cập nhật chỉ phụ thêm. Đối với mỗi bản cập nhật tài khoản, phiên bản mới được lưu trữ trong AppendVec.

Có thể tối ưu hóa các bản cập nhật trong một lần fork bằng cách trả về một tham chiếu có thể thay đổi cho tài khoản đã được lưu trữ trong một đợt fork. Ngân hàng đã theo dõi quyền truy cập đồng thời của các tài khoản và đảm bảo rằng việc ghi vào một fork tài khoản cụ thể sẽ không đồng thời với việc đọc vào một tài khoản tại Fork đó. Để hỗ trợ hoạt động này, AppendVec nên triển khai chức năng này:

```text
fn get_mut(&self, index: u64) -> &mut T;
```

API này cho phép truy cập đồng thời có thể thay đổi vào một vùng bộ nhớ tại `index`. Nó dựa vào Ngân hàng để đảm bảo quyền truy cập độc quyền vào chỉ số đó.

## Thu gom rác

Khi các tài khoản được cập nhật, chúng sẽ chuyển đến phần cuối của AppendVec. Sau khi hết dung lượng, một AppendVec mới có thể được tạo và các bản cập nhật có thể được lưu trữ ở đó. Cuối cùng, các tham chiếu đến AppendVec cũ hơn sẽ biến mất vì tất cả các tài khoản đã được cập nhật và AppendVec cũ có thể bị xóa.

Để tăng tốc quá trình này, bạn có thể di chuyển các Tài khoản chưa được cập nhật gần đây sang một AppendVec mới. Hình thức thu thập rác này có thể được thực hiện mà không yêu cầu các khóa độc quyền đối với bất kỳ cấu trúc dữ liệu nào ngoại trừ cập nhật chỉ mục.

Việc triển khai ban đầu để thu gom rác là khi tất cả các tài khoản trong AppendVec trở thành phiên bản cũ, nó sẽ được sử dụng lại. Các tài khoản không được cập nhật hoặc di chuyển xung quanh sau khi được thêm vào.

## Phục hồi chỉ mục

Mỗi chuỗi ngân hàng có quyền truy cập độc quyền vào các tài khoản trong khi nối, vì các khóa tài khoản không thể được giải phóng cho đến khi dữ liệu được cam kết. Nhưng không có thứ tự ghi rõ ràng giữa các tệp AppendVec riêng biệt. Để tạo một thứ tự, chỉ mục duy trì một bộ đếm phiên bản ghi nguyên tử. Mỗi phần thêm vào AppendVec ghi lại số phiên bản ghi chỉ mục cho phần phụ đó trong mục nhập cho Tài khoản trong AppendVec.

Để khôi phục chỉ mục, tất cả các tệp AppendVec có thể được đọc theo bất kỳ thứ tự nào và phiên bản ghi mới nhất cho mỗi lần fork phải được lưu trữ trong chỉ mục.

## Ảnh chụp nhanh

Để chụp nhanh, các tệp được ánh xạ bộ nhớ cơ bản trong AppendVec cần phải được chuyển vào ổ đĩa cứng cứng. Chỉ mục cũng có thể được ghi ra ổ đĩa cứng.

## Hiệu suất

- Việc ghi-chỉ thêm vào rất nhanh. Các SSD và NVME, cũng như tất cả các cấu trúc dữ liệu nhân cấp hệ điều hành, cho phép phần phụ chạy nhanh như băng thông PCI hoặc NVMe sẽ cho phép \(2,700 MB/s\).
- Mỗi luồng phát lại và luồng ngân hàng ghi đồng thời vào AppendVec của chính nó.
- Mỗi AppendVec có thể được lưu trữ trên một NVMe riêng biệt.
- Mỗi luồng phát lại và luồng ngân hàng có quyền truy cập đọc đồng thời vào tất cả các AppendVec mà không chặn ghi.
- Chỉ mục yêu cầu một khóa ghi độc quyền để ghi. Hiệu suất một luồng đối với các bản cập nhật HashMap theo thứ tự là 10 phút mỗi giây.
- Giai đoạn Ngân hàng và Phát lại nên sử dụng 32 luồng cho mỗi NVMe. Các NVMe có hiệu suất tối ưu với 32 trình đọc hoặc ghi đồng thời.
