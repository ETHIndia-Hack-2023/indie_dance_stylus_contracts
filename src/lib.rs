// Only run this as a WASM if the export-abi feature is not set.
#![cfg_attr(not(feature = "export-abi"), no_main)]
extern crate alloc;

mod erc20;

/// Initializes a custom, global allocator for Rust programs compiled to WASM.
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

use std::ops::{Add, AddAssign, Mul};

use crate::erc20::{Erc20, Erc20Params};
use alloy_primitives::address;
/// Import the Stylus SDK along with alloy primitive types for use in our program.
use stylus_sdk::{
    alloy_primitives::Address, alloy_primitives::U256, block, call::RawCall, contract,
    function_selector, msg, prelude::*,
};

const OWNER: Address = address!("05221C4fF9FF91F04cb10F46267f492a94571Fa9");

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

pub struct InDanceParams;

impl Erc20Params for InDanceParams {
    const NAME: &'static str = "In Dance";
    const SYMBOL: &'static str = "IND";
    const DECIMALS: u8 = 18;
}

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
        #[borrow] // Allows erc20 to access Weth's storage and make calls
        Erc20<InDanceParams> erc20;

    }
}

impl InDance {
    fn _claim(&mut self, user: Address) -> Result<U256, Vec<u8>> {
        let claim = self.claims.get(msg::sender());
        let last_claimed_time = self.last_claimed.get(user);

        let mut claim_pending = U256::ZERO;

        if last_claimed_time.eq(&U256::ZERO) {
            return Ok(U256::ZERO);
        }
        {
            let user_tokens_per_minute = self.tokens_per_minute.get(user);
            let time_diff = (U256::from(block::timestamp()).checked_sub(last_claimed_time))
                .ok_or("IBT1")?;

            claim_pending.add_assign(time_diff.mul(user_tokens_per_minute));
        }

        if claim_pending.eq(&U256::ZERO) {
            return Err("NTC1".into());
        }

        // Update last claim time
        self.last_claimed
            .setter(user)
            .set(U256::from(block::timestamp()));

        let total_claim = claim.add(claim_pending);

        self.erc20.mint(user, total_claim);

        Ok(total_claim)
    }
}

#[external]
#[inherit(Erc20<InDanceParams>)]
impl InDance {
    pub fn get_claimable(&self, user: Address) -> Result<U256, Vec<u8>> {
        let claim = self.claims.get(user);
        let last_claimed_time = self.last_claimed.get(user);

        let mut claim_pending = U256::ZERO;

        if last_claimed_time.gt(&U256::ZERO) {
            let user_tokens_per_minute = self.tokens_per_minute.get(user);
            let time_diff = (U256::from(block::timestamp()).checked_sub(last_claimed_time))
                .ok_or("EBT")?;

            claim_pending.add_assign(time_diff.mul(user_tokens_per_minute));
        }

        Ok(claim.add(claim_pending))
    }
    pub fn claim(&mut self) -> Result<U256, Vec<u8>> {
        return self._claim(msg::sender());
    }

    pub fn get_dance_floor(
        &self,
        user: Address,
        floor_id: U256,
    ) -> Result<[(U256, U256); 9], Vec<u8>> {
        let mut result: [(U256, U256); 9] = [(U256::ZERO, U256::ZERO); 9];

        let user_floors = self.floors.get(user);
        let floor = user_floors.get(floor_id);
        let dancers = &floor.ok_or("NOF")?.dancers;

        for i in 0..9 {
            let dancer = dancers.get(i).ok_or("NOD")?;
            result[i] = (dancer.level.get(), dancer.params.get());
        }

        Ok(result)
    }

    pub fn buy_dancer(&mut self, level: U256) -> Result<(), Vec<u8>> {
        self._claim(msg::sender())?;

        let (level, coins_per_minute, price): (u32, u32, u32) =
            DANCERS_TO_BUY[level.byte(0) as usize];

        // price = price * 10 ** 18
        let price = U256::from(price)
            .checked_mul(U256::from(10).pow(U256::from(18)))
            .ok_or("OVF")?;

        // Receveing tokens from user
        self.erc20
            .burn(msg::sender(), price)
            .err()
            .ok_or("NEB1")?;

        let last_floor_id = self.last_floor_ids.get(msg::sender());

        let mut last_dancer_id = 0;
        for i in 0..9 {
            if self
                .floors
                .get(msg::sender())
                .get(last_floor_id)
                .ok_or("NLFI")?
                .dancers
                .get(i)
                .ok_or("NI")?
                .level
                .gt(&U256::ZERO)
            {
                last_dancer_id += 1;
            } else {
                break;
            }
        }

        if last_dancer_id == 9 {
            return Err("FULL".into());
        }

        let mut user_floors_setter = (self.floors).setter(msg::sender());
        let mut last_floor = user_floors_setter
            .get_mut(last_floor_id)
            .ok_or("NLF")?;

        let old_floor_tokens_per_minute = last_floor.base_tokens_per_minute.get();
        let coins_per_minute_delta = U256::from(coins_per_minute)
            .checked_mul(U256::from(10).pow(U256::from(18)))
            .ok_or("OVF")?;

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

        let mut dancer_setter = last_floor
            .dancers
            .setter(last_dancer_id)
            .ok_or("NOST")?;

        dancer_setter.level.set(U256::from(level));
        dancer_setter.params.set(U256::from(1));

        return Ok(());
    }

    pub fn buy_floor(&mut self) -> Result<(), Vec<u8>> {
        let last_floor_id = self.last_floor_ids.get(msg::sender());

        if last_floor_id.gt(&U256::ZERO) {
            let price = U256::from(FLOOR_PRICE)
                .checked_mul(U256::from(10).pow(U256::from(18)))
                .ok_or("OVF")?;

            // Receveing tokens from user
            self.erc20
                .burn(msg::sender(), price)
                .err()
                .ok_or("NEB")?;
        }


        let mut last_dancer_id = 0;

        if last_floor_id.gt(&U256::ZERO) {
            for i in 0..9 {
                if self
                    .floors
                    .get(msg::sender())
                    .get(last_floor_id)
                    .ok_or("Err1")?
                    .dancers
                    .get(i)
                    .ok_or("Err2")?
                    .level
                    .gt(&U256::ZERO)
                {
                    last_dancer_id += 1;
                } else {
                    break;
                }
            }
        }
        if last_dancer_id < 9 {
            // Last floor should be full to buy a new one
            return Err("NOTF".into());
        }

        // Updating last floor id
        self.last_floor_ids
            .setter(msg::sender())
            .set(last_floor_id.add(U256::from(1)));

        self.floors.setter(msg::sender()).grow();

        return Ok(());
    }
}
