use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use cosmwasm_std::{Addr, Uint128};
use cw20::{Cw20Coin, MinterResponse};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    
    /// The list of assets in the basket
    pub assets: Vec<(
        // token_address: 
        Addr,
        // token_weight: 
        Uint128,
        //min_profit_basis_points: 
        Uint128,
        //max_lptoken_amount: 
        Uint128,
        //stable_token: 
        bool,
        //shortable_token: 
        bool,
        //oracle_address: 
        Addr,
        //backup_oracle_address: 
        Addr
    )>,

    /// Name of Basket
	pub name: String,
	/// fee for non-stable asset perp
	pub tax_basis_points: Uint128,
	/// fee for stable asset perp
	pub stable_tax_basis_points: Uint128,
	/// base fee for mint/burning lp token
	pub mint_burn_basis_points: Uint128,
	/// base fee for swap
	pub swap_fee_basis_points: Uint128,
	/// base fee for swaping between stable assets 
	pub stable_swap_fee_basis_points: Uint128, 
	/// references position fees, not for funding rate, nor for getting in/out of a position
	pub margin_fee_basis_points: Uint128, 
	/// fee for getting liquidated, goes to liquidator in USD
	pub liquidation_fee_usd: Uint128,
	/// prevents gaming of oracle with hourly trades
	pub min_profit_time: Uint128,
	/// cache the total weights of the assets	
	pub total_weights: Uint128,
	/// account that can make changes to the exchange
	pub admin: Addr,
    /// The token contract code ID used for the tokens in the pool
    pub token_code_id: u64,

}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Increment {},
    Decrement {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    // Basket returns the basket as a json-encoded string
    Basket {},
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CountResponse {
    pub count: u8,
}


/// This structure describes the parameters used for creating a token contract.
/// TokenContract InstantiateMsg
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct InstantiateLpMsg {
    /// Token name
    pub name: String,
    /// Token symbol
    pub symbol: String,
    /// The amount of decimals the token has
    pub decimals: u8,
    /// Initial token balances
    pub initial_balances: Vec<Cw20Coin>,
    /// Minting controls specified in a [`MinterResponse`] structure
    pub mint: Option<MinterResponse>,
}