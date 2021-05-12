---
title: Requisitos de Validador
---

## Hardware

- Recomendaciones de CPU
  - Recomendamos una CPU con el mayor número de núcleos posible. Las CPUs AMD Threadripper o Intel Server \(Xeon\) están bien.
  - Recomendamos AMD Threadripper ya que obtienes un mayor número de núcleos para la paralelización en comparación con Intel.
  - Threadripper también tiene una ventaja de coste por núcleo y un mayor número de carriles PCIe en comparación con la pieza equivalente de Intel. PoH \(Proof of History\) se basa en sha256 y Threadripper también soporta instrucciones de hardware sha256.
- Tamaño SSD y estilo E/S \(SATA vs NVMe/M.2\) para un validador
  - Ejemplo mínimo - Samsung 860 Evo 2TB
  - Ejemplo de rango medio - Samsung 860 Evo 4TB
  - Ejemplo de alto nivel - Samsung 860 Evo 4TB
- GPUs
  - Mientras que un nodo sólo de la CPU puede mantenerse al día con la red de recolección inicial, una vez que aumente el rendimiento de las transacciones, los GPU serán necesarios
  - ¿Qué tipo de GPU?
    - Recomendamos Nvidia Turing y volta familia GPUs 1660ti a 2080ti serie consumidor GPU serie o Tesla servidor GPUs.
    - Actualmente no soportamos OpenCL y por lo tanto no soportamos AMD GPUs. Tenemos una recompensa para que alguien nos haga llegar a OpenCL. ¿Interesado? [Mira nuestro GitHub.](https://github.com/solana-labs/solana)
- Consumo de energía
  - Consumo de energía aproximado para un nodo validador que ejecuta un AMD Threadripper 3950x y 2x 2080Ti GPUs es 800-1000W.

### Configuraciones preconfiguradas

Estas son nuestras recomendaciones para las especificaciones de máquinas de baja, mediana y de alto nivel:

|                         | Baja                | Media                  | Alta                   | Notas                                                                                         |
| :---------------------- | :------------------ | :--------------------- | :--------------------- | :-------------------------------------------------------------------------------------------- |
| CPU                     | AMD Ryzen 3950x     | AMD Threadripper 3960x | AMD Threadripper 3990x | Considere una placa base de 10 Gb con tantos carriles PCIe y ranuras de m.2 como sea posible. |
| RAM                     | 32GB                | 64GB                   | 128GB                  |                                                                                               |
| Unidad Ledger           | Samsung 860 Evo 2TB | Samsung 860 Evo 4TB    | Samsung 860 Evo 4TB    | O equivalente SSD                                                                             |
| Unidad de clientes\(s\) | Ninguna             | Samsung 970 Pro 1TB    | 2x Samsung 970 Pro 1TB |                                                                                               |
| GPU                     | Nvidia 1660ti       | Nvidia 2080 Ti         | 2x Nvidia 2080 Ti      | Cualquier número de GPUs con capacidad cuda son soportados en plataformas Linux.              |

## Máquinas virtuales en plataformas en la nube

Aunque puedes ejecutar un validador en una plataforma de computación en la nube, puede que no sea rentable a largo plazo.

Sin embargo, puede ser conveniente ejecutar nodos api sin votación en instancias de VM para su propio uso interno. Este caso de uso incluye intercambios y servicios basados en Solana.

De hecho, los nodos oficiales de la API mainnet-beta se ejecutan actualmente (octubre de 2020) en instancias GCE `n1-standard-32` (32 vCPUs, 120 GB de memoria) con 2048 GB de SSD para mayor comodidad operativa.

Para otras plataformas en la nube, seleccione tipos de instancia con especificaciones similares.

También ten en cuenta que el uso del tráfico de Internet con egresos puede resultar alto, especialmente para el caso de que se ejecuten validadores con stake.

## Docker

Validador en ejecución para clústeres en vivo (incluyendo mainnet-beta) dentro de Docker no es recomendado y generalmente no soportado. Esto se debe a las preocupaciones de la sobrecarga del contenedor general del docker y la degradación del rendimiento resultante a menos que esté especialmente configurado.

Utilizamos docker sólo para fines de desarrollo.

## Software

- Construimos y corremos en Ubuntu 18.04. Algunos usuarios han tenido problemas al ejecutar en Ubuntu 16.04
- Vea [Instalar Solana](../cli/install-solana-cli-tools.md) para la versión actual de software de Solana.

Asegúrate de que la máquina utilizada no está detrás de un NAT residencial, para evitar problemas de travesía. Una máquina alojada en la nube funciona mejor. **Asegúrese de que los puertos IP de 8000 a 10000 no están bloqueados para el tráfico de entrada y salida de Internet.** Para más información sobre el reenvío de puertos con respecto a las redes residenciales, vea [este documento](http://www.mcs.sdsmt.edu/lpyeatt/courses/314/PortForwardingSetup.pdf).

Los binarios precompilados están disponibles para Linux x86_64 \(Se recomienda Ubuntu 18.04\). Los usuarios de MacOS o WSL pueden construir desde el código fuente.

## Requisitos de GPU

Se requiere CUDA para hacer uso del GPU en su sistema. Los binarios de Solana proporcionados se construyen en Ubuntu 18.04 con [CUDA Toolkit 10.1 update 1](https://developer.nvidia.com/cuda-toolkit-archive). Si su máquina está usando una versión CUDA diferente, entonces necesitará reconstruir desde el código fuente.
