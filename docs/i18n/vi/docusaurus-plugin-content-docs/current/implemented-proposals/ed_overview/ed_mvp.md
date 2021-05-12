---
title: Thiết kế kinh tế MVP
---

**Có thể thay đổi.**

Các phần trước, được nêu trong [Tổng quan về thiết kế kinh tế](ed_overview.md), mô tả tầm nhìn dài hạn về một nền kinh tế Solana bền vững. Tất nhiên, chúng tôi không mong đợi việc triển khai cuối cùng hoàn toàn phù hợp với những gì đã được mô tả ở trên. Chúng tôi dự định tham gia đầy đủ với các bên liên quan với mạng trong suốt các giai đoạn triển khai \(tức là pre-testnet, testnet, mainnet\) để đảm bảo hệ thống hỗ trợ và đại diện cho các lợi ích khác nhau của những người tham gia mạng. Tuy nhiên, bước đầu tiên hướng tới mục tiêu này là phác thảo một số tính năng kinh tế MVP mong muốn có sẵn cho những người tham gia pre-testnet và testnet sớm. Dưới đây là bản phác thảo sơ bộ chức năng kinh tế cơ bản để từ đó có thể phát triển một hệ thống chức năng và hoàn chỉnh hơn.

## Đặc điểm kinh tế MVP

- Vòi để cung cấp các SOL của testnet cho validator để staking và phát triển ứng dụng.
- Cơ chế mà những validator được thưởng thông qua lạm phát mạng.
- Khả năng ủy quyền mã thông báo cho các validator node
- Validator đặt phí hoa hồng dựa trên lãi suất từ ​​các mã thông báo được ủy quyền.
