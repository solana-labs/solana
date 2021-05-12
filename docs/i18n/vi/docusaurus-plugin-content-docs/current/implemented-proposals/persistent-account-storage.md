---
title: Lưu trữ tài khoản liên tục
---

## Lưu trữ tài khoản liên tục

The set of accounts represent the current computed state of all the transactions that have been processed by a validator. Mỗi validator cần duy trì toàn bộ tập hợp này. Each block that is proposed by the network represents a change to this set, and since each block is a potential rollback point, the changes need to be reversible.

Lưu trữ liên tục như NVME rẻ hơn từ 20 đến 40 lần so với DDR. The problem with persistent storage is that write and read performance is much slower than DDR. Care must be taken in how data is read or written to. Both reads and writes can be split between multiple storage drives and accessed in parallel. This design proposes a data structure that allows for concurrent reads and concurrent writes of storage. Writes are optimized by using an AppendVec data structure, which allows a single writer to append while allowing access to many concurrent readers. The accounts index maintains a pointer to a spot where the account was appended to every fork, thus removing the need for explicit checkpointing of state.

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

## Garbage collection

As accounts get updated, they move to the end of the AppendVec. Once capacity has run out, a new AppendVec can be created and updates can be stored there. Eventually references to an older AppendVec will disappear because all the accounts have been updated, and the old AppendVec can be deleted.

To speed up this process, it's possible to move Accounts that have not been recently updated to the front of a new AppendVec. This form of garbage collection can be done without requiring exclusive locks to any of the data structures except for the index update.

The initial implementation for garbage collection is that once all the accounts in an AppendVec become stale versions, it gets reused. The accounts are not updated or moved around once appended.

## Index Recovery

Each bank thread has exclusive access to the accounts during append, since the accounts locks cannot be released until the data is committed. But there is no explicit order of writes between the separate AppendVec files. To create an ordering, the index maintains an atomic write version counter. Each append to the AppendVec records the index write version number for that append in the entry for the Account in the AppendVec.

To recover the index, all the AppendVec files can be read in any order, and the latest write version for every fork should be stored in the index.

## Snapshots

To snapshot, the underlying memory-mapped files in the AppendVec need to be flushed to disk. The index can be written out to disk as well.

## Performance

- Việc ghi-chỉ thêm vào rất nhanh. Các SSD và NVME, cũng như tất cả các cấu trúc dữ liệu nhân cấp hệ điều hành, cho phép phần phụ chạy nhanh như băng thông PCI hoặc NVMe sẽ cho phép \(2,700 MB/s\).
- Mỗi luồng phát lại và luồng ngân hàng ghi đồng thời vào AppendVec của chính nó.
- Mỗi AppendVec có thể được lưu trữ trên một NVMe riêng biệt.
- Mỗi luồng phát lại và luồng ngân hàng có quyền truy cập đọc đồng thời vào tất cả các AppendVec mà không chặn ghi.
- Chỉ mục yêu cầu một khóa ghi độc quyền để ghi. Hiệu suất một luồng đối với các bản cập nhật HashMap theo thứ tự là 10 phút mỗi giây.
- Giai đoạn Ngân hàng và Phát lại nên sử dụng 32 luồng cho mỗi NVMe. Các NVMe có hiệu suất tối ưu với 32 trình đọc hoặc ghi đồng thời.
