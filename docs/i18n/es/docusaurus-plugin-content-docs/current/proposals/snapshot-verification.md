---
title: Verificación de Snapshot
---

## Problema

La verificación Snapshot de los estados de la cuenta se implementa, pero el hash bancario del Snapshot que se utiliza para verificar es falsificable.

## Solución

Mientras un validador está procesando transacciones para llegar al clúster desde el Snapshot, usar transacciones de voto entrantes y la calculadora de compromiso para confirmar que el clúster se está construyendo sobre el hash bancario snapshotted. Una vez alcanzado el nivel de compromiso de umbral, acepte el snapshot como válido y comience a votar.
