// Only run this as a WASM if the export-abi feature is not set.
#![cfg_attr(not(feature = "export-abi"), no_main)]
extern crate alloc;

/// Initializes a custom, global allocator for Rust programs compiled to WASM.
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

use std::ops::{Add, AddAssign, Mul};

use alloy_primitives::address;
/// Import the Stylus SDK along with alloy primitive types for use in our program.
use stylus_sdk::{
    alloy_primitives::Address, alloy_primitives::U256, block, call::RawCall, contract,
    function_selector, msg, prelude::*,
};

const OWNER: Address = address!("05221C4fF9FF91F04cb10F46267f492a94571Fa9");
const TOKEN: Address = address!("05221C4fF9FF91F04cb10F46267f492a94571Fa9");

const LEVEL_NUMS: usize = 5;

const FLOOR_PRICE: usize = 100;

// Level - coins_per_minute - price
const DANCERS_TO_BUY: [(u32, u32, u32); LEVEL_NUMS] = [
    (1, 2, 5),
    (2, 5, 15),
    (3, 10, 25),
    (4, 100, 200),
    (5, 5000, 10000),
];

sol_storage! {
    pub struct Dancer {
        uint256 level;
        uint256 params;
    }

    pub struct DanceFloor {
        Dancer[9] dancers;
        uint256 base_tokens_per_minute;
    }

    #[entrypoint]
    pub struct InDance {
        mapping(address => DanceFloor[]) floors;
        mapping(address => uint256) last_floor_ids;
        mapping(address => uint256) last_claimed;
        mapping(address => uint256) claims;
        mapping(address => uint256) tokens_per_minute;

    }
}

#[external]
impl InDance {
    pub fn get_claimable(&self, user: Address) -> Result<U256, Vec<u8>> {
        let claim = self.claims.get(user);
        let last_claimed_time = self.last_claimed.get(user);

        let mut claim_pending = U256::ZERO;

        if last_claimed_time.gt(&U256::ZERO) {
            let user_tokens_per_minute = self.tokens_per_minute.get(user);
            let time_diff = (U256::from(block::timestamp()).checked_sub(last_claimed_time))
                .ok_or("Incorrect block time")
                .unwrap();

            claim_pending.add_assign(time_diff.mul(user_tokens_per_minute));
        }

        Ok(claim.add(claim_pending))
    }
    pub fn claim(&mut self) -> Result<U256, Vec<u8>> {
        return self._claim(msg::sender());
    }

    fn _claim(&mut self, user: Address) -> Result<U256, Vec<u8>> {
        let claim = self.claims.get(msg::sender());
        let last_claimed_time = self.last_claimed.get(user);

        let mut claim_pending = U256::ZERO;

        if last_claimed_time.eq(&U256::ZERO) {
            return Err("Nothing to claim".into());
        }
        {
            let user_tokens_per_minute = self.tokens_per_minute.get(user);
            let time_diff = (U256::from(block::timestamp()).checked_sub(last_claimed_time))
                .ok_or("Incorrect block time")
                .unwrap();

            claim_pending.add_assign(time_diff.mul(user_tokens_per_minute));
        }

        if claim_pending.eq(&U256::ZERO) {
            return Err("Noting to claim(2)".into());
        }

        // Update last claim time
        self.last_claimed
            .setter(user)
            .set(U256::from(block::timestamp()));

        // Minting tokens to user
        let selector = function_selector!("transfer(address)");
        let data = [&selector[..], &user.into_array()].concat();
        RawCall::new()
            .call(TOKEN, &data)
            .unwrap_or("Token transfer error".into());

        Ok(claim.add(claim_pending))
    }

    pub fn get_dance_floor(
        &self,
        user: Address,
        floor_id: U256,
    ) -> Result<[(U256, U256); 9], Vec<u8>> {
        let mut result: [(U256, U256); 9] = [(U256::ZERO, U256::ZERO); 9];

        let user_floors = self.floors.get(user);
        let floor = user_floors.get(floor_id);
        let dancers = &floor.unwrap().dancers;

        for i in 0..9 {
            let dancer = dancers.get(i).unwrap();
            result[i] = (dancer.level.get(), dancer.params.get());
        }

        Ok(result)
    }

    pub fn buy_dancer(&mut self, level: U256) -> Result<(), Vec<u8>> {
        self._claim(msg::sender()).unwrap();

        let (level, coins_per_minute, price): (u32, u32, u32) =
            DANCERS_TO_BUY[level.byte(0) as usize];

        // price = price * 10 ** 18
        let price = U256::from(price)
            .checked_mul(U256::from(10).pow(U256::from(18)))
            .ok_or("Overflow")
            .unwrap();
        // Receiving tokens to contract
        let selector = function_selector!("transferFrom(address,address,uint256)");
        let data = [
            &selector[..],
            &msg::sender().into_array(),
            &contract::address().into_array(),
            &price.to_be_bytes::<32>(),
        ]
        .concat();
        RawCall::new()
            .call(TOKEN, &data)
            .unwrap_or("Token transfer error".into());

        let last_floor_id = self.last_floor_ids.get(msg::sender());

        let mut last_dancer_id = 0;
        for i in 0..9 {
            if self
                .floors
                .get(msg::sender())
                .get(last_floor_id)
                .unwrap()
                .dancers
                .get(i)
                .unwrap()
                .level
                .gt(&U256::ZERO)
            {
                last_dancer_id += 1;
            } else {
                break;
            }
        }

        if last_dancer_id == 9 {
            return Err("Full dancefloor. Buy a new one".into());
        }

        let mut user_floors_setter = (self.floors).setter(msg::sender());
        let mut last_floor = user_floors_setter.get_mut(last_floor_id).unwrap();

        let old_floor_tokens_per_minute = last_floor.base_tokens_per_minute.get();
        let coins_per_minute_delta = U256::from(coins_per_minute)
            .checked_mul(U256::from(10).pow(U256::from(18)))
            .ok_or("Overflow")
            .unwrap();

        last_floor.base_tokens_per_minute.set(
            coins_per_minute_delta
                .add(old_floor_tokens_per_minute)
                .clone(),
        );

        let current_user_tokens_per_minute = self.tokens_per_minute.get(msg::sender());

        // Incrementing user tokens per minute
        self.tokens_per_minute
            .setter(msg::sender())
            .set(current_user_tokens_per_minute.add(coins_per_minute_delta));

        let mut dancer_setter = last_floor.dancers.setter(last_dancer_id).unwrap();

        dancer_setter.level.set(U256::from(level));
        dancer_setter.params.set(U256::from(1));

        return Ok(());
    }

    pub fn buy_floor(&mut self) -> Result<(), Vec<u8>> {
        let price = U256::from(FLOOR_PRICE)
            .checked_mul(U256::from(10).pow(U256::from(18)))
            .ok_or("Overflow")
            .unwrap();

        let selector = function_selector!("transferFrom(address,address,uint256)");
        let data = [
            &selector[..],
            &msg::sender().into_array(),
            &contract::address().into_array(),
            &price.to_be_bytes::<32>(),
        ]
        .concat();
        RawCall::new()
            .call(TOKEN, &data)
            .unwrap_or("Token transfer error".into());

        let last_floor_id = self.last_floor_ids.get(msg::sender());

        let mut last_dancer_id = 0;

        for i in 0..9 {
            if self
                .floors
                .get(msg::sender())
                .get(last_floor_id)
                .unwrap()
                .dancers
                .get(i)
                .unwrap()
                .level
                .gt(&U256::ZERO)
            {
                last_dancer_id += 1;
            } else {
                break;
            }
        }

        if last_dancer_id < 9 {
            // Last floor should be full to buy a new one
            return Err("Dancefloor is not full".into());
        }

        // Updating last floor id
        self.last_floor_ids
            .setter(msg::sender())
            .set(last_floor_id.add(U256::from(1)));

        self.floors.setter(msg::sender()).grow();

        return Ok(());
    }
}
