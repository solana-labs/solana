---
title: 지침 내부 검사
---

## 문제

일부 스마트 컨트랙트 프로그램은 해당 명령이 사전 컴파일 된 기능에서 특정 데이터의 검증을 수행 할 수 있으므로 주어진 메시지에 다른 명령이 있는지 확인하려고 할 수 있습니다. (예제는 secp256k1 \ \_instruction 참조).

## 해결책

프로그램이 내부에서 메시지의 명령 데이터와 현재 명령의 인덱스를 참조하고 수신 할 수있는 새 sysvar Sysvar1nstructions1111111111111111111111111을 추가합니다.

이 데이터를 추출하는 두 가지 도우미 함수를 사용할 수 있습니다.

```
fn load_current_index (instruction_data : & [u8])-> u16;
fn load_instruction_at (instruction_index : usize, instruction_data : & [u8])-> Result <Instruction>;
```

런타임은이 특수 명령어를 인식하고 이에 대한 메시지 명령어 데이터를 직렬화하고 현재 명령어 색인을 작성하면 bpf 프로그램이 여기에서 필요한 정보를 추출 할 수 있습니다.

참고 : bincode는 네이티브 코드에서 약 10 배 더 느리고 현재 BPF 명령어 제한을 초과하기 때문에 명령어의 사용자 지정 직렬화가 사용됩니다.
