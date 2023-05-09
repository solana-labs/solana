//! `Pubkey` Javascript interface
#![cfg(target_arch = "wasm32")]
#![allow(non_snake_case)]
use {
    crate::{pubkey::*, wasm::display_to_jsvalue},
    js_sys::{Array, Uint8Array},
    wasm_bindgen::{prelude::*, JsCast},
};

fn js_value_to_seeds_vec(array_of_uint8_arrays: &[JsValue]) -> Result<Vec<Vec<u8>>, JsValue> {
    let vec_vec_u8 = array_of_uint8_arrays
        .iter()
        .filter_map(|u8_array| {
            u8_array
                .dyn_ref::<Uint8Array>()
                .map(|u8_array| u8_array.to_vec())
        })
        .collect::<Vec<_>>();

    if vec_vec_u8.len() != array_of_uint8_arrays.len() {
        Err("Invalid Array of Uint8Arrays".into())
    } else {
        Ok(vec_vec_u8)
    }
}

#[wasm_bindgen]
impl Pubkey {
    /// Create a new Pubkey object
    ///
    /// * `value` - optional public key as a base58 encoded string, `Uint8Array`, `[number]`
    #[wasm_bindgen(constructor)]
    pub fn constructor(value: JsValue) -> Result<Pubkey, JsValue> {
        if let Some(base58_str) = value.as_string() {
            base58_str.parse::<Pubkey>().map_err(display_to_jsvalue)
        } else if let Some(uint8_array) = value.dyn_ref::<Uint8Array>() {
            Pubkey::try_from(uint8_array.to_vec())
                .map_err(|err| JsValue::from(format!("Invalid Uint8Array pubkey: {err:?}")))
        } else if let Some(array) = value.dyn_ref::<Array>() {
            let mut bytes = vec![];
            let iterator = js_sys::try_iter(&array.values())?.expect("array to be iterable");
            for x in iterator {
                let x = x?;

                if let Some(n) = x.as_f64() {
                    if n >= 0. && n <= 255. {
                        bytes.push(n as u8);
                        continue;
                    }
                }
                return Err(format!("Invalid array argument: {:?}", x).into());
            }
            Pubkey::try_from(bytes)
                .map_err(|err| JsValue::from(format!("Invalid Array pubkey: {err:?}")))
        } else if value.is_undefined() {
            Ok(Pubkey::default())
        } else {
            Err("Unsupported argument".into())
        }
    }

    /// Return the base58 string representation of the public key
    pub fn toString(&self) -> String {
        self.to_string()
    }

    /// Check if a `Pubkey` is on the ed25519 curve.
    pub fn isOnCurve(&self) -> bool {
        self.is_on_curve()
    }

    /// Checks if two `Pubkey`s are equal
    pub fn equals(&self, other: &Pubkey) -> bool {
        self == other
    }

    /// Return the `Uint8Array` representation of the public key
    pub fn toBytes(&self) -> Box<[u8]> {
        self.0.clone().into()
    }

    /// Derive a Pubkey from another Pubkey, string seed, and a program id
    pub fn createWithSeed(base: &Pubkey, seed: &str, owner: &Pubkey) -> Result<Pubkey, JsValue> {
        Pubkey::create_with_seed(base, seed, owner).map_err(display_to_jsvalue)
    }

    /// Derive a program address from seeds and a program id
    pub fn createProgramAddress(
        seeds: Box<[JsValue]>,
        program_id: &Pubkey,
    ) -> Result<Pubkey, JsValue> {
        let seeds_vec = js_value_to_seeds_vec(&seeds)?;
        let seeds_slice = seeds_vec
            .iter()
            .map(|seed| seed.as_slice())
            .collect::<Vec<_>>();

        Pubkey::create_program_address(seeds_slice.as_slice(), program_id)
            .map_err(display_to_jsvalue)
    }

    /// Find a valid program address
    ///
    /// Returns:
    /// * `[PubKey, number]` - the program address and bump seed
    pub fn findProgramAddress(
        seeds: Box<[JsValue]>,
        program_id: &Pubkey,
    ) -> Result<JsValue, JsValue> {
        let seeds_vec = js_value_to_seeds_vec(&seeds)?;
        let seeds_slice = seeds_vec
            .iter()
            .map(|seed| seed.as_slice())
            .collect::<Vec<_>>();

        let (address, bump_seed) = Pubkey::find_program_address(seeds_slice.as_slice(), program_id);

        let result = Array::new_with_length(2);
        result.set(0, address.into());
        result.set(1, bump_seed.into());
        Ok(result.into())
    }
}
