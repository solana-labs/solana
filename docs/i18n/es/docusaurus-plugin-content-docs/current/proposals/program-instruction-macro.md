# Macro de Instrucción de Programa

## Problema

Actualmente, la inspección de una transacción en la cadena requiere depender de una biblioteca de decodificación del lado del cliente, específica del lenguaje, para analizar la instrucción. Si los métodos rpc pudieran devolver los detalles de las instrucciones decodificadas, estas soluciones personalizadas serían innecesarias.

Podemos deserializar los datos de las instrucciones utilizando el enum de instrucciones de un programa, pero la decodificación de la lista de claves de la cuenta en identificadores legibles para el ser humano requiere un análisis manual. Si los métodos rpc pudieran devolver los detalles de las instrucciones decodificadas, estas soluciones personalizadas serían innecesarias. Nuestros enums de Instrucción actuales tienen esa información de cuenta, pero solo en documentos variantes.

Del mismo modo, tenemos funciones constructoras de instrucciones que duplican casi toda la información del enum, pero no podemos generar ese constructor a partir de la definición del enum porque la lista de referencias de cuentas está en los comentarios del código.

Además, los documentos de instrucción pueden variar entre implementaciones, ya que no hay mecanismo para asegurar la consistencia.

## Solución propuesta

Mueva los datos de los comentarios de código a los atributos, de tal manera que los constructores puedan ser generados, e incluya toda la documentación de la definición de enum.

Aquí hay un ejemplo de un enum de Instrucción usando el nuevo formato de cuentas:

```rust,ignore
#[instructions(test_program::id())]
pub enum TestInstruction {
    /// Transfer lamports
    #[accounts(
        from_account(SIGNER, WRITABLE, desc = "Funding account"),
        to_account(WRITABLE, desc = "Recipient account"),
    )]
    Transfer {
        lamports: u64,
    },

    /// Provide M of N required signatures
    #[accounts(
        data_account(WRITABLE, desc = "Data account"),
        signers(SIGNER, multiple, desc = "Signer"),
    )]
    Multisig,

    /// Consumes a stored nonce, replacing it with a successor
    #[accounts(
        nonce_account(SIGNER, WRITABLE, desc = "Nonce account"),
        recent_blockhashes_sysvar(desc = "RecentBlockhashes sysvar"),
        nonce_authority(SIGNER, optional, desc = "Nonce authority"),
    )]
    AdvanceNonceAccount,
}
```

Un ejemplo de la TestInstruction generada con docs:

```rust,ignore
pub enum TestInstruction {
    /// Transfer lamports
    ///
    /// * Accounts expected by this instruction:
    ///   0. `[WRITABLE, SIGNER]` Funding account
    ///   1. `[WRITABLE]` Recipient account
    Transfer {
        lamports: u64,
    },

    /// Provide M of N required signatures
    ///
    /// * Accounts expected by this instruction:
    ///   0. `[WRITABLE]` Data account
    ///   * (Multiple) `[SIGNER]` Signers
    Multisig,

    /// Consumes a stored nonce, replacing it with a successor
    ///
    /// * Accounts expected by this instruction:
    ///   0. `[WRITABLE, SIGNER]` Nonce account
    ///   1. `[]` RecentBlockhashes sysvar
    ///   2. (Optional) `[SIGNER]` Nonce authority
    AdvanceNonceAccount,
}
```

Constructores generados:

```rust,ignore
/// Transfer lamports
///
/// * `from_account` - `[WRITABLE, SIGNER]` Funding account
/// * `to_account` - `[WRITABLE]` Recipient account
pub fn transfer(from_account: Pubkey, to_account: Pubkey, lamports: u64) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(from_pubkey, true),
        AccountMeta::new(to_pubkey, false),
    ];
    Instruction::new_with_bincode(
        test_program::id(),
        &SystemInstruction::Transfer { lamports },
        account_metas,
    )
}

/// Provide M of N required signatures
///
/// * `data_account` - `[WRITABLE]` Data account
/// * `signers` - (Multiple) `[SIGNER]` Signers
pub fn multisig(data_account: Pubkey, signers: &[Pubkey]) -> Instruction {
    let mut account_metas = vec![
        AccountMeta::new(nonce_pubkey, false),
    ];
    for pubkey in signers.iter() {
        account_metas.push(AccountMeta::new_readonly(pubkey, true));
    }

    Instruction::new_with_bincode(
        test_program::id(),
        &TestInstruction::Multisig,
        account_metas,
    )
}

/// Consumes a stored nonce, replacing it with a successor
///
/// * nonce_account - `[WRITABLE, SIGNER]` Nonce account
/// * recent_blockhashes_sysvar - `[]` RecentBlockhashes sysvar
/// * nonce_authority - (Optional) `[SIGNER]` Nonce authority
pub fn advance_nonce_account(
    nonce_account: Pubkey,
    recent_blockhashes_sysvar: Pubkey,
    nonce_authority: Option<Pubkey>,
) -> Instruction {
    let mut account_metas = vec![
        AccountMeta::new(nonce_account, false),
        AccountMeta::new_readonly(recent_blockhashes_sysvar, false),
    ];
    if let Some(pubkey) = authorized_pubkey {
        account_metas.push(AccountMeta::new_readonly*nonce_authority, true));
    }
    Instruction::new_with_bincode(
        test_program::id(),
        &TestInstruction::AdvanceNonceAccount,
        account_metas,
    )
}

```

Enumeración generada de TestInstructionVerbose:

```rust,ignore
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum TestInstruction {
    /// Transfer lamports
    Transfer {
        /// Funding account
        funding_account: u8

        /// Recipient account
        recipient_account: u8

        lamports: u64,
    },

    /// Provide M of N required signatures
    Multisig {
        data_account: u8,
        signers: Vec<u8>,
    },

    /// Consumes a stored nonce, replacing it with a successor
    AdvanceNonceAccount {
        nonce_account: u8,
        recent_blockhashes_sysvar: u8,
        nonce_authority: Option<u8>,
    }
}

impl TestInstructionVerbose {
    pub fn from_instruction(instruction: TestInstruction, account_keys: Vec<u8>) -> Self {
        match instruction {
            TestInstruction::Transfer { lamports } => TestInstructionVerbose::Transfer {
                funding_account: account_keys[0],
                recipient_account: account_keys[1],
                lamports,
            }
            TestInstruction::Multisig => TestInstructionVerbose::Multisig {
                data_account: account_keys[0],
                signers: account_keys[1..],
            }
            TestInstruction::AdvanceNonceAccount => TestInstructionVerbose::AdvanceNonceAccount {
                nonce_account: account_keys[0],
                recent_blockhashes_sysvar: account_keys[1],
                nonce_authority: &account_keys.get(2),
            }
        }
    }
}

```

## Consideraciones

1. **Campos con nombre** - Dado que el enum resultante de Verbose construye variantes con campos con nombre, cualquier campo sin nombre en la variante original de Instrucción necesitará tener nombres generados. Por lo tanto, sería mucho más sencillo si todos los campos de enumeraciones de instrucción se convirtieran en tipos con nombre, en lugar de tuplas sin nombre. Esto parece que vale la pena hacerlo de todos modos, añadiendo más precisión a las variantes y permitiendo una documentación real (para que los desarrolladores no tengan que hacer [esto](https://github.com/solana-labs/solana/blob/3aab13a1679ba2b7846d9ba39b04a52f2017d3e0/sdk/src/system_instruction.rs#L140). Esto causará un poco de movimiento en nuestra base de código actual, pero no mucho.
2. **Listas de cuentas variables** - Este enfoque ofrece un par de opciones para las listas de cuentas variables. En primer lugar, se pueden añadir cuentas opcionales y etiquetarlas con la palabra clave `opcional`. Sin embargo, actualmente solo una cuenta opcional es soportada por instrucción. Será necesario añadir datos adicionales a la instrucción para soportar los múltiplos, permitiendo identificar qué cuentas están presentes cuando se incluyen algunas pero no todas. En segundo lugar, las cuentas que comparten las mismas características pueden añadirse como un conjunto, etiquetado con la palabra clave `múltiple`. Al igual que las cuentas opcionales, sólo se admite un conjunto de cuentas múltiples por instrucción ( opcional y múltiple no pueden coexistir). Las instrucciones más complejas que no pueden ser acomodadas por `opcional` o `múltiple`, y que requieren una lógica para averiguar orden/representación de la cuenta, probablemente deberían convertirse en instrucciones separadas.
