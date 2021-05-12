---
title: Khung kiểm tra cụm
---

Tài liệu này đề xuất Khung kiểm tra cụm \(CTF\). CTF là một khai thác thử nghiệm cho phép các thử nghiệm thực thi với một cụm cục bộ, trong quá trình hoặc một cụm đã triển khai.

## Động lực

Mục tiêu của CTF là cung cấp một khuôn khổ để viết các bài kiểm tra độc lập với vị trí và cách thức mà cụm được triển khai. Các hồi quy có thể được ghi lại trong các thử nghiệm này và các thử nghiệm có thể được chạy trên các cụm đã triển khai để xác minh việc triển khai. Trọng tâm của các thử nghiệm này phải là tính ổn định của cụm, sự đồng thuận, khả năng chịu lỗi, tính ổn định của API.

Các bài kiểm tra phải xác minh một lỗi hoặc một tình huống duy nhất và phải được viết với số lượng đường ống bên trong ít nhất được tiếp xúc với bài kiểm tra.

## Tổng quan về Thiết kế

Các bài kiểm tra được cung cấp một điểm đầu vào, đó là một `contact_info::ContactInfo` cấu trúc và một keypair đã được cấp vốn.

Mỗi node trong cụm được định cấu hình với `validator::ValidatorConfig` tại thời điểm khởi động. Tại thời điểm khởi động, cấu hình này chỉ định bất kỳ cấu hình cụm bổ sung nào cần thiết cho quá trình kiểm tra. Cụm sẽ khởi động với cấu hình khi nó được chạy trong quá trình hoặc trong trung tâm dữ liệu.

Sau khi khởi động, thử nghiệm sẽ khám phá cụm thông qua một gossip và định cấu hình bất kỳ hành vi thời gian chạy nào thông qua validator RPC.

## Giao diện Thử nghiệm

Mỗi bài thử nghiệm CTF bắt đầu với một điểm vào không rõ ràng và một keypair được tài trợ. Việc thử nghiệm không nên phụ thuộc vào cách cụm được triển khaiv à phải có thể thực hiện tất cả các chức năng của cụm thông qua các giao diện có sẵn công khai.

```text
use crate::contact_info::ContactInfo;
use solana_sdk::signature::{Keypair, Signer};
pub fn test_this_behavior(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    num_nodes: usize,
)
```

## Khám phá Cụm

Khi bắt đầu thử nghiệm, cụm đã được thiết lập và được kết nối đầy đủ. Thử nghiệm có thể khám phá hầu hết các node có sẵn trong vài giây.

```text
use crate::gossip_service::discover_nodes;

// Discover the cluster over a few seconds.
let cluster_nodes = discover_nodes(&entry_point_info, num_nodes);
```

## Cấu hình Cụm

Để kích hoạt các tình huống cụ thể, cụm cần được khởi động với các cấu hình đặc biệt. Các cấu hình này có thể được ghi lại trong `validator::ValidatorConfig`.

Ví dụ:

```text
et mut validator_config = ValidatorConfig::default();
validator_config.rpc_config.enable_validator_exit = true;
let local = LocalCluster::new_with_config(
                num_nodes,
                10_000,
                100,
                &validator_config
                );
```

## Cách thiết kế một bài thử nghiệm mới

Ví dụ, có một lỗi cho thấy rằng cụm không thành công khi nó tràn ngập bởi các node gossip được quảng cáo không hợp lệ. Thư viện gossip và giao thức của chúng tôi có thể thay đổi, nhưng cụm vẫn cần duy trì khả năng chống chọi ồ ạt của các node gossip được quảng cáo không hợp lệ.

Cấu hình dịch vụ RPC:

```text
let mut validator_config = ValidatorConfig::default();
validator_config.rpc_config.enable_rpc_gossip_push = true;
validator_config.rpc_config.enable_rpc_gossip_refresh_active_set = true;
```

Kết nối RPC và viết một bài thử nghiệm mới:

```text
pub fn test_large_invalid_gossip_nodes(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    num_nodes: usize,
) {
    let cluster = discover_nodes(&entry_point_info, num_nodes);

    // Poison the cluster.
    let client = create_client(entry_point_info.client_facing_addr(), VALIDATOR_PORT_RANGE);
    for _ in 0..(num_nodes * 100) {
        client.gossip_push(
            cluster_info::invalid_contact_info()
        );
    }
    sleep(Durration::from_millis(1000));

    // Force refresh of the active set.
    for node in &cluster {
        let client = create_client(node.client_facing_addr(), VALIDATOR_PORT_RANGE);
        client.gossip_refresh_active_set();
    }

    // Verify that spends still work.
    verify_spends(&cluster);
}
```
