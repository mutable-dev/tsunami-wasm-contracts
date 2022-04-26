use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128, QuerierWrapper, Uint256, StdResult, Querier};
use std::convert::{TryFrom, TryInto};
use cw_storage_plus::{Item, Map};
use crate::error::ContractError;
use crate::asset::{Asset, AssetInfo};
use crate::msg::{InstantiateMsg, InstantiateAssetInfo};
use crate::contract::{safe_price_to_Uint128, safe_u128_to_i64, USD_VALUE_PRECISION, safe_i64_to_u128};
use crate::querier::{query_supply, query_token_precision};
use pyth_sdk_terra::{PriceFeed, Price, PriceIdentifier, PriceStatus, query_price_feed};
use std::cfg;
use phf::{phf_map};

/// Basket of assets
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Basket {
	/// Assets
	pub assets: Vec<BasketAsset>,
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
	/// account that can make changes to the exchange
	pub admin: Addr,
	/// LP token address
	pub lp_token_address: Addr
}

/// Represents whitelisted assets on the dex
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BasketAsset {
	/// AssetInfo
	pub info: AssetInfo,
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

impl BasketAsset {
	pub fn new(asset_info: InstantiateAssetInfo) -> Self {
	
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

		BasketAsset {
			/// Static asset info about the token
			info: asset_info.info,
			/// The weight of this token in the LP 
			token_weight: asset_info.weight,
			/// min about of profit a position needs to be in to take profit before time
			min_profit_basis_points: asset_info.min_profit_basis_points,
			/// maximum amount of this token that can be in the pool
			max_asset_amount: asset_info.max_asset_amount,
			/// Flag for whether this is a stable token
			stable_token: asset_info.is_asset_stable,
			/// Flag for whether this asset is shortable
			shortable_token: asset_info.is_asset_shortable,
			/// The cumulative funding rate for the asset
			cumulative_funding_rate,
			/// Last time the funding rate was updated
			last_funding_time,
			/// Account with price oracle data on the asset
			oracle_address: asset_info.oracle_address,
			/// Backup account with price oracle data on the asset
			backup_oracle_address: asset_info.backup_oracle_address,
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

pub struct AumResult {
	pub aum: Uint128,
	pub price: i64,
	pub exponent: i32,
}

impl Basket {
	pub fn new(
		assets: Vec<BasketAsset>,
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
			admin: msg.admin.clone(),
			lp_token_address: Addr::unchecked(""), // This is fixed in reply
		}
	}

	pub fn get_total_weights(&self) -> Uint128 {
		let mut total_weights = Uint128::zero();
		for asset in &self.assets{
			total_weights += asset.token_weight;
		}
		total_weights
	}

	pub fn get_basket_assets(&self, asset_infos: &Vec<AssetInfo>) -> Vec<BasketAsset> {
		let mut v: Vec<BasketAsset> = vec![];
		for asset in asset_infos.iter() {
			v.push(
				self.assets
					.iter()
					.find(|basket_asset| basket_asset.info.equal(&asset))
					.expect("an asset was not found in the basket")
					.clone()
			)
		}
		v
	}

	// CHECK: that we should take the value of the token account as AUM and not the general reserves from the
	// available asset account
	pub fn calculate_aum(
		&self,
		querier: &QuerierWrapper,
		//reserve_basket_asset_info: &AssetInfo,
	) -> Result<Price, ContractError> {
		// let mut aum = Uint128::new(0);
		// let mut precise_price = 0;
		// let mut exponent =  1;
		// let mut current_basket_asset: &BasketAsset = &self.assets[0];
		// let reserve_asset_denom: String;
		// match reserve_basket_asset_info {
		// 		AssetInfo::NativeToken{ denom } => reserve_asset_denom = denom.to_string(),
		// 		_ => {
		// 				return Err(ContractError::NonNativeAssetAssertion);
		// 		}
		// }

		// Build amounts: input to price_basket
		let tokens: Vec<(Asset, Price)> = self.get_pools().iter().map(|x| x.clone()).zip(self.get_prices(querier)?).collect();
		let amounts: &[(Price, i64, i32)] = &tokens
			.iter()
			.map(|(asset, price)| (*price, safe_u128_to_i64(asset.amount.u128()).unwrap(), -(query_token_precision(querier, &asset.info).unwrap() as i32)))
			.collect::<Vec<(Price, i64, i32)>>();

		// Construct aum Price result
		Ok(Price::price_basket(
			amounts, 
			USD_VALUE_PRECISION,
		).unwrap())
	}
	

	/// TODO: Calculates total number of lp tokens
	pub fn total_tokens(&self, querier: &QuerierWrapper, info: AssetInfo) -> StdResult<Uint128> {

		// TODO: implement to_addr()
		let contract_addr = Addr::unchecked("0x0000000000000000000000000000000000000000");//info.to_addr();

		if cfg!(feature = "test") {
			Ok(Uint128::from(1_u8))
		} else {
			Ok(query_supply(querier, contract_addr)?)
		}
	}

	/// Calculates usd amount to withdraw. Reduce fees elsewhere
	pub fn withdraw_amount(&self, lp_amount: Uint128, info: AssetInfo, querier: &QuerierWrapper) -> Result<Uint128, ContractError> {
		
		// Calculate aum in USD, in units of USD_VALUE_PRECISION
		let aum_value: Uint128 = safe_price_to_Uint128(self.calculate_aum(querier)?);

		// Calculate amount of lp token to mint in USD, in units of USD_VALUE_PRECISION
		let refund_value: Uint128 = lp_amount.multiply_ratio(aum_value, self.total_tokens(querier, info)?);

		Ok(refund_value)
	}

	/// TODO: Gathers all `Asset`s in basket.
	pub fn get_pools(&self) -> Vec<Asset> {
		let mut v = vec![];
		for asset in &self.assets {
			v.push(Asset {
				info: asset.info.clone(),
				amount: Uint128::from(1_u32) // TODO: GATHER POOL BALANCES, REPLACE THIS DUMMY
			})
		}
		v
	}

	/// TODO: Get actual oracle price feeds
	pub fn get_price_feeds(&self, querier: &QuerierWrapper) -> Result<Vec<PriceFeed>, ContractError> {

		if cfg!(feature = "test")
		{
			let mut v = vec![];
			for asset in &self.assets {
				let dummy_time = 0;
				let dummy_exponent = 0;
				let dummy_max_num_publishers = 5;
				let dummy_num_publishers = 3;
				let dummy_exponent = 0;
				let dummy_price = 1_000_000;
				let dummy_conf = 1_000;
				let dummy_ema_price = 1_000_000;
				let dummy_ema_conf = 1_000;
				let dummy_prev_price = 1_000_000;
				let dummy_prev_conf = 1_000_000;
				let dummy_pref_publish_time = -10;
				v.push(
					PriceFeed::new(
						PriceIdentifier::new([0; 32]),
						PriceStatus::Trading,
						dummy_time,
						dummy_exponent,
						dummy_max_num_publishers,
						dummy_num_publishers,
						PriceIdentifier::new([0; 32]),
						dummy_price,
						dummy_conf,
						dummy_ema_price,
						dummy_ema_conf,
						dummy_prev_price,
						dummy_prev_conf,
						dummy_pref_publish_time,
					)
				);
			}
			return Ok(v)
		} else {
			let mut v = vec![];
			for asset in &self.assets {
				// TODO: ADD REAL PRICE IDENTIFIERS
				let dummy_identifier = PriceIdentifier::new([0; 32]);
				v.push(
					match query_price_feed(querier, asset.oracle_address.to_string(), dummy_identifier) {
						Ok(price_feed_response) => price_feed_response.price_feed,
						_ => return Err(ContractError::OracleQueryFailed)
					}
				);
			}
			return Ok(v)
		}
	}

	// This uses `get_price_feeds` and goes a step further to unwrap `Price`s.
	pub fn get_prices(&self, querier: &QuerierWrapper) -> Result<Vec<Price>, ContractError> {
		let price_feeds: Vec<PriceFeed> = match self.get_price_feeds(querier) {
			Ok(price_feeds) => price_feeds,
			_ => return Err(ContractError::OracleQueryFailed)
		};
		Ok(price_feeds
			.iter()
			.map(|&x| x.get_current_price().unwrap())
			.collect())
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

pub struct TickerData {
	pub testnet_address: &'static str,
	pub mainnet_address: &'static str,
	pub testnet_price_feed: &'static str,
	pub mainnet_price_feed: &'static str,
}

pub static ASSET_MAP: phf::Map<&'static str,  TickerData> = phf_map! {
	"ust" => TickerData {
		testnet_address: "0x0000000000000000000000000000000000000000",
		mainnet_address: "0x0000000000000000000000000000000000000000",
		testnet_price_feed: "0x026d1f1cf9f1c0ee92eb55696d3bd2393075b611c4f468ae5b967175edc4c25c",
		mainnet_price_feed: "0x0000000000000000000000000000000000000000",
	},
	"luna" => TickerData {
		testnet_address: "0x0000000000000000000000000000000000000000",
		mainnet_address: "0x0000000000000000000000000000000000000000",
		testnet_price_feed: "0x6de025a4cf28124f8ea6cb8085f860096dbc36d9c40002e221fc449337e065b2",
		mainnet_price_feed: "0x0000000000000000000000000000000000000000",
	},
};

pub static PYTH_CONTRACTS : phf::Map<&'static str, &'static str> = phf_map! {
	"mainnet" => "0x0000000000000000000000000000000000000000",
	"testnet" => "terra1hdc8q4ejy82kd9w7wj389dlul9z5zz9a36jflh",
};

