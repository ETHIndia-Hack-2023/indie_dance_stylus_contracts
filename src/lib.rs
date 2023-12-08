
// Only run this as a WASM if the export-abi feature is not set.
#![cfg_attr(not(feature = "export-abi"), no_main)]
extern crate alloc;

/// Initializes a custom, global allocator for Rust programs compiled to WASM.
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

use alloy_primitives::address;
/// Import the Stylus SDK along with alloy primitive types for use in our program.
use stylus_sdk::{alloy_primitives::U256, alloy_primitives::Address, prelude::*, function_selector, msg, call::RawCall, contract};


const OWNER: Address = address!("05221C4fF9FF91F04cb10F46267f492a94571Fa9");
const TOKEN: Address = address!("05221C4fF9FF91F04cb10F46267f492a94571Fa9");


sol_storage! {
    pub struct Dancer {
        uint256 level;
        uint256 params;
    }

    pub struct DanceFloor {
        Dancer[9] dancers;
        uint256 base_tokens_per_second;
    }

    #[entrypoint]
    pub struct InDance {
        mapping(address => DanceFloor[]) floors;
        mapping(address => uint256) last_floor_ids;
    }
}

#[external]
impl InDance {

    pub fn get_dance_floor(&self, user: Address, floor_id: U256) -> Result<[(U256, U256); 9], Vec<u8>> {
        let mut result: [(U256, U256); 9] = [(U256::from(0), U256::from(0)); 9];

        let user_floors = self.floors.get(user);
        let floor = user_floors.get(floor_id);
        let dancers  = &floor.unwrap().dancers;

        for i in 0..9 {
            let dancer = dancers.get(i).unwrap();
            result[i] = (dancer.level.get(), dancer.params.get());
        }

        Ok(result)
    }

    pub fn buy_dancer(&mut self, id: U256) -> Result<(), Vec<u8>> {
        let price = U256::from(10).checked_mul(U256::from(10).pow(U256::from(18))).ok_or("Overflow").unwrap();
        let selector = function_selector!("transferFrom(address,address,uint256)");
        let data = [
            &selector[..],
            &msg::sender().into_array(),
            &contract::address().into_array(),
            &price.to_be_bytes::<32>(),
        ].concat();
        RawCall::new().call(TOKEN, &data).unwrap_or("Token transfer error".into());

        let last_floor_id = self.last_floor_ids.get(msg::sender());

        let mut last_dancer_id = 0;
        for i in 0..9 {
            if self.floors.get(msg::sender()).get(last_floor_id).unwrap().dancers.get(i).unwrap().level.gt(&U256::from(0)) {
                last_dancer_id += 1;
            }
        }

        if last_dancer_id == 9 {
            return Err("Full dancefloor. Buy a new one".into());
        }

        let mut user_floors_setter = (self.floors)
            .setter(msg::sender());

        let mut binding = user_floors_setter.get_mut(last_floor_id).unwrap();

        let mut dancer_setter = binding.dancers.setter(last_dancer_id).unwrap();

        dancer_setter.level.set(U256::from(1));
        dancer_setter.params.set(U256::from(1));

        return Ok(());
    }
}
