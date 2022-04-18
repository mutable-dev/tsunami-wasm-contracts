use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use cosmwasm_std::{Addr, Uint128, Decimal};
use cw20::{Cw20Coin, MinterResponse, Cw20ReceiveMsg};
use crate::asset::{Asset, AssetInfo};
use crate::state::BasketAsset;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    
    /// The list of assets in the basket
    pub assets: Vec<InstantiateAssetInfo>,
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
    DepositLiquidity {
        assets: Vec<Asset>,
        slippage_tolerance: Option<Decimal>,
        receiver: Option<String>,
    },
    Receive { msg: Cw20ReceiveMsg },
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

#[derive(PartialEq,Clone,Default)]
pub struct MsgInstantiateContractResponse {
    // message fields
    pub contract_address: String,
    pub data: Vec<u8>,
}

/// This structure describes the parameters used for instantiating
/// the assets in an LP
/// InstantiateAssetInfo
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateAssetInfo {
    /// Asset Info
    pub info: AssetInfo,
    /// Token address
    pub address: Addr,
    /// Token weight
    pub weight: Uint128,
    /// The minimum amount of profit a position with the asset needs
    /// to be in before closing otherwise, no profit
    pub min_profit_basis_points: Uint128,
    /// Maximum amount of asset that can be held in the LP
    pub max_asset_amount: Uint128,
    /// If the asset is a stable token
    pub is_asset_stable: bool,
    /// If the asset can be shorted 
    pub is_asset_shortable: bool,
    /// Address of the oracle for the asset 
    pub oracle_address: Addr,
    /// Backup oracle address for the asset
    pub backup_oracle_address: Addr,
}


/// This structure describes the parameters used for a message 
/// creating a LP Token. 
/// InstantiateLpMsg
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

/// This structure describes a CW20 hook message.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    /// Swap a given amount of asset
    Swap {
        belief_price: Option<Decimal>,
        max_spread: Option<Decimal>,
        to: Option<String>,
        ask_asset: AssetInfo,
    },
    /// Withdraw liquidity from the pool
    WithdrawLiquidity { basket_asset: BasketAsset },
}


