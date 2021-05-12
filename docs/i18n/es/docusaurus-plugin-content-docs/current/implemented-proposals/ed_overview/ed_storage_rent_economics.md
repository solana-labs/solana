---
title: Economía del alquiler del almacén
---

Cada transacción que se presenta al Ledger de Solana impone costos. Las tasas de transacción pagadas por el remitente y cobradas por un validador, en teoría, dan cuenta de los costes agudos, transaccionales, de validar y añadir esos datos al ledger. En este proceso no se tiene en cuenta el almacenamiento a medio plazo del estado del ledger activo, mantenido necesariamente por el conjunto de validadores rotativos. Este tipo de almacenamiento impone costes no sólo a los validadores, sino también a la red en general, ya que el estado activo aumenta, al igual que la transmisión de datos y la sobrecarga de validación. Para tener en cuenta estos costes, describimos aquí nuestro diseño preliminar e implementación de alquiler de almacenamiento.

El Almacenamiento de alquiler se puede pagar con uno de los dos métodos:

Método 1: Establécelo y olvídalo

Con este enfoque, las cuentas con depósitos de alquiler garantizados por un valor de dos años están exentas de los gastos de alquiler de la red. Manteniendo este equilibrio mínimo, la red más amplia se beneficia de una liquidez reducida y el titular de la cuenta puede confiar en que su `Account::data` se mantendrá para el acceso / uso continuo.

Método 2: Pagar por byte

Si una cuenta tiene menos de dos años de alquiler depositado, la red cobra el alquiler por época, en crédito para la próxima época. Este alquiler se reduce a una tasa especificada en génesis, en lamports por kilobyte-año.

Para obtener información sobre los detalles técnicos de la implementación de este diseño, consulte la sección [Rent](../rent.md).
