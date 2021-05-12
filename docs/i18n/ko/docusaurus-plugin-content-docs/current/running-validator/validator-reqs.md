---
title: 유효성 검사기 요구 사항
---

## 하드웨어

- -CPU 권장 사항 -가능한 한 많은 수의 코어가있는 CPU를 권장합니다.
  - -CPU 권장 사항 -가능한 한 많은 수의 코어가있는 CPU를 권장합니다. AMD Threadripper 또는 Intel Server \ (Xeon \) CPU는 괜찮습니다.
  - -Intel에 비해 병렬화를 위해 더 많은 수의 코어를 확보 할 수 있으므로 AMD Threadripper를 권장합니다.
  - -Threadripper는 또한 동등한 Intel 부품에 비해 코어 당 비용 이점과 더 많은 PCIe 레인을 제공합니다. 역사증명 \ (Proof of History \)는 sha256을 기반으로하며 Threadripper는 sha256 하드웨어 명령어도 지원합니다.
- 소비 전력
  - 대략적인 전력 소비 AMD의 Threadripper 3950x 및 배 2080Ti GPU는 800-1000W입니다 실행 발리 노드.
  - 삼성 860 Evo 4TB
  - 삼성 860 Evo 4TB
- GPU 요구 사항
  - GPU는 -는 CPU 만 노드가 증가 처리량 거래하면, 초기 공회전 네트워크와 계속 할 수 있지만, GPU는 필요하다
  - 어떤 종류의 GPU의?
    - -Nvidia Turing 및 volta 제품군 GPU 1660ti ~ 2080ti 시리즈 소비자 GPU 또는 Tesla 시리즈 서버 GPU를 권장합니다.
    - -현재 OpenCL을 지원하지 않으므로 AMD GPU를 지원하지 않습니다. 누군가 우리를 OpenCL로 이식 할 수있는 현상금이 있습니다. 관심이 있으십니까? \[우리의 GitHub의를 확인하십시오.\] (https://github.com/solana-labs/solana)
- 소비 전력
  - 대략적인 전력 소비 AMD의 Threadripper 3950x 및 배 2080Ti GPU는 800-1000W입니다 실행 발리 노드.

### 사전 구성된 설정

다음은 저, 중 및 고급 시스템 사양에 대한 권장 사항입니다.

|                   | 저가형             | 중간 끝                   | 하이 엔드                  | 참고                                                 |
|:----------------- |:--------------- |:---------------------- |:---------------------- |:-------------------------------------------------- |
| CPU               | AMD Ryzen 3950x | AMD Threadripper 3960x | AMD Threadripper 3990x | 가능한 한 많은 PCIe 레인과 m.2 슬롯이있는 10Gb 지원 마더 보드를 고려하십시오. |
| RAM               | 32GB            | 64GB                   | 128GB                  |                                                    |
| 원장 드라이브           | 삼성 860 Evo 2TB  | 삼성 860 Evo 4TB         | 삼성 860 Evo 4TB         | 또는 동등한 SSD                                         |
| 계정 드라이브 \ (s \) | 없음              | 삼성 970 Pro 1TB         | 2x Samsung 970 Pro 1TB |                                                    |
| GPU               | Nvidia 1660ti   | Nvidia 2080 Ti         | Nvidia 2080 Ti 2 개     | Linux 플랫폼에서는 원하는 수의 cuda 지원 GPU가 지원됩니다.            |

## 클라우드 플랫폼의 가상 머신

다음은 저, 중 및 고급 시스템 사양에 대한 권장 사항입니다.

그러나 자체 내부 사용을 위해 VM 인스턴스에서 비 투표 API 노드를 실행하는 것이 편리 할 수 ​​있습니다. 이 사용 사례에는 Solana에 구축 된 교환 및 서비스가 포함됩니다.

실제로 공식 메인 넷-베타 API 노드는 현재 (2020 년 10 월) 운영 편의를 위해 2048GB SSD가있는 GCE`n1-standard-32` (32 개의 vCPU, 120GB 메모리) 인스턴스에서 실행되고 있습니다.

실제로 공식 메인 넷-베타 API 노드는 현재 (2020 년 10 월) 운영 편의를 위해 2048GB SSD가있는 GCE`n1-standard-32` (32 개의 vCPU, 120GB 메모리) 인스턴스에서 실행되고 있습니다.

다른 클라우드 플랫폼의 경우 유사한 사양의 인스턴스 유형을 선택하십시오.

## Docker Docker

내부의 라이브 클러스터 (mainnet-beta 포함)에 대한 유효성 검사기를 실행하는 것은 권장되지 않으며 일반적으로 지원되지 않습니다. 이는 일반 도커의 컨테이너화 오버 헤드 및 특별히 구성하지 않는 한 성능 저하로 인한 문제 때문입니다.

우리는 개발 목적으로 만 docker를 사용합니다.

## 소프트웨어

- -Ubuntu 18.04에서 빌드하고 실행합니다. 일부 사용자는 Ubuntu 16.04에서 실행할 때 문제가 발생 했습니다.
- 현재 Solana 소프트웨어 릴리스는 \[Installing Solana\] (../ cli / install-solana-cli-tools.md)를 참조하십시오.

NAT 통과 문제를 방지하려면 사용되는 시스템이 주거용 NAT 뒤에 있지 않은지 확인하십시오. 클라우드 호스팅 시스템이 가장 잘 작동합니다. ** 인터넷 인바운드 및 아웃 바운드 트래픽에 대해 IP 포트 8000 ~ 10000이 차단되지 않도록합니다. ** 가정용 네트워크와 관련된 포트 전달에 대한 자세한 내용은 \[이 문서\] (http://www.mcs.sdsmt.edu)를 참조하십시오. /lpyeatt/courses/314/PortForwardingSetup.pdf).

사전 빌드 된 바이너리는 Linux x86_64 \ (Ubuntu 18.04 권장 \)에서 사용할 수 있습니다. MacOS 또는 WSL 사용자는 소스에서 빌드 할 수 있습니다.

## GPU 요구 사항

CUDA는 시스템에서 GPU를 사용하는 데 필요합니다. 제공된 Solana 릴리스 바이너리는 \[CUDA Toolkit 10.1 업데이트 1\] (https://developer.nvidia.com/cuda-toolkit-archive)을 사용하여 Ubuntu 18.04에 빌드됩니다. 컴퓨터에서 다른 CUDA 버전을 사용하는 경우 소스에서 다시 빌드해야합니다.
