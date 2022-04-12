use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

use crate::msg::{InstantiateMsg, InstantiateAssetInfo};

/// Basket of assets
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Basket {
	/// Assets
	pub assets: Vec<Asset>,
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
}

/// Represents whitelisted assets on the dex
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Asset {
	/// Token address of the available asset
	pub token_address: Addr,
	/// the decimals for the token
	pub token_decimals: Uint128,
	/// The weight of this token in the LP 
	pub token_weight: Uint128,
	/// min about of profit a position needs to be in to take profit before time
	pub min_profit_basis_points: Uint128,
	/// maximum amount of this token that can be in the pool
	pub max_asset_amount: Uint128,
	/// Flag for whether this is a stable token
	pub stable_token: bool,
	/// Flag for whether this asset is shortable
	pub shortable_token: bool,
	/// The cumulative funding rate for the asset
	pub cumulative_funding_rate: Uint128,
	/// Last time the funding rate was updated
	pub last_funding_time: Uint128,
	/// Account with price oracle data on the asset
	pub oracle_address: Addr,
	/// Backup account with price oracle data on the asset
	pub backup_oracle_address: Addr,
	/// Global size of shorts denominated in kind
	pub global_short_size: Uint128,
	/// Represents the total outstanding obligations of the protocol (position - size) for the asset
	pub net_protocol_liabilities: Uint128,
	/// Assets that are reserved and having positions trading against them
	pub occupied_reserves: Uint128,
	/// Represents how much in reserves the pool owns of the available asset from fees
	pub fee_reserves: Uint128,
	/// Represents the unoccupied + occupied amount of assets in the pool for trading 
	/// does not include fee_reserves
	pub pool_reserves: Uint128,
}

impl Asset {
	pub fn new(assetInfo: InstantiateAssetInfo) -> Self {
		// TODO: query CW20 for decimals
		let token_decimals = Uint128::from(8_u32);

		// TODO: Fix these, if needed.
		let cumulative_funding_rate = Uint128::default();
		let last_funding_time = Uint128::default();
		let net_protocol_liabilities = Uint128::default();
		let global_short_size = Uint128::default();
		let occupied_reserves = Uint128::default();
		let fee_reserves = Uint128::default();
		let pool_reserves = Uint128::default();

		Asset {
			/// Token address of the available asset
			token_address: assetInfo.address,
			/// the decimals for the token
			token_decimals,
			/// The weight of this token in the LP 
			token_weight: assetInfo.weight,
			/// min about of profit a position needs to be in to take profit before time
			min_profit_basis_points: assetInfo.min_profit_basis_points,
			/// maximum amount of this token that can be in the pool
			max_asset_amount: assetInfo.max_asset_amount,
			/// Flag for whether this is a stable token
			stable_token: assetInfo.is_asset_stable,
			/// Flag for whether this asset is shortable
			shortable_token: assetInfo.is_asset_shortable,
			/// The cumulative funding rate for the asset
			cumulative_funding_rate,
			/// Last time the funding rate was updated
			last_funding_time,
			/// Account with price oracle data on the asset
			oracle_address: assetInfo.oracle_address,
			/// Backup account with price oracle data on the asset
			backup_oracle_address: assetInfo.backup_oracle_address,
			/// Global size of shorts denominated in kind
			global_short_size,
			/// Represents the total outstanding obligations of the protocol (position - size) for the asset
			net_protocol_liabilities,
			/// Assets that are reserved and having positions trading against them
			occupied_reserves,
			/// Represents how much in reserves the pool owns of the available asset from fees
			fee_reserves,
			/// Represents the unoccupied + occupied amount of assets in the pool for trading 
			/// does not include fee_reserves
			pool_reserves,
		}
	}
}


impl Basket {
	pub fn new(
		assets: Vec<Asset>,
		msg: &InstantiateMsg,
	) -> Self {
		Basket {
			assets,
			name: msg.name.clone(),
			tax_basis_points: msg.tax_basis_points,
			stable_tax_basis_points: msg.stable_tax_basis_points,
			mint_burn_basis_points: msg.mint_burn_basis_points,
			swap_fee_basis_points: msg.swap_fee_basis_points,
			stable_swap_fee_basis_points: msg.stable_swap_fee_basis_points,
			margin_fee_basis_points: msg.margin_fee_basis_points,
			liquidation_fee_usd: msg.liquidation_fee_usd,
			min_profit_time: msg.min_profit_time,
			total_weights: msg.total_weights,
			admin: msg.admin.clone(),
		}
	}
}


/// Represents whitelisted assets on the dex
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Oracle {
	/// This is a fake price
	pub price: Uint128,
	/// Boolean
	pub valid: bool,
}

pub const BASKET: Item<Basket> = Item::new("basket");
