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
use pyth_sdk_terra::{PriceFeed, Price, PriceIdentifier, PriceStatus, query_price_feed, PriceFeedResponse};
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
	pub oracle: OracleInterface,

	/// Backup account with price oracle data on the asset
	pub backup_oracle: OracleInterface,

	/// Global size of shorts denominated in kind
	pub global_short_size: Uint128,

	/// Represents the total outstanding obligations of the protocol (position - size) for the asset
	pub net_protocol_liabilities: Uint128,

	/// Assets that are reserved and having positions trading against them
	pub occupied_reserves: Uint128,

	/// Represents how much in reserves the pool owns of the available asset from fees
	pub fee_reserves: Uint128,

	/// Represents the unoccupied + occupied amount of assets in the pool for trading 
	/// Does not include fee_reserves
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
			oracle: asset_info.oracle,
			/// Backup account with price oracle data on the asset
			backup_oracle: asset_info.backup_oracle,
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
	) -> Result<Price, ContractError> {

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

		// Calculate value of lp_amount lp tokens in USD, in units of USD_VALUE_PRECISION
		let redeem_value: Uint128 = lp_amount.multiply_ratio(aum_value, self.total_tokens(querier, info)?);

		Ok(redeem_value)
	}

	/// TODO: Completely deprecate this method's usage in contract.rs and then remove it from here
	/// It's better to just work with the assets field of a basket directly
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
		let mut v = vec![];
		for asset in &self.assets {
			v.push(asset.oracle.get_price_feed(querier)?);
		}
		
		Ok(v)
	}

	// This uses `get_price_feeds` and goes a step further to unwrap `Price`s.
	pub fn get_prices(&self, querier: &QuerierWrapper) -> Result<Vec<Price>, ContractError> {
		let mut v = vec![];
		for asset in &self.assets {
			v.push(asset.oracle.get_price(querier)?);
		}

		Ok(v)
	}
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum OracleInterface {
	Pyth {
		addr: Addr, 
		price_id: PriceIdentifier
	},
	Stub {
		price: i64,
		expo: i32,
	}
}

impl OracleInterface {
	/// Construct new Pyth oracle source for an asset
	pub fn from_pyth(addr: Addr, price_id: PriceIdentifier) -> Self {
		Self::Pyth{ addr, price_id }
	}

	/// Construct a dummy oracle that will yield the given price
	pub fn from_dummy(price: i64, expo: i32) -> Self {
		Self::Stub{ price, expo }
	}

	pub fn get_price_feed(&self, querier: &QuerierWrapper) -> StdResult<PriceFeed> {
		match self {
			Self::Pyth{ addr, price_id } => {
				let price_feed = query_price_feed(querier, addr.to_string(), *price_id)?.price_feed;

				Ok(price_feed)
			},

			// Create a dummy price feed wrapper for the stub price
			Self::Stub{ price, expo } => Ok(PriceFeed::new(
				PriceIdentifier::new([0; 32]),
				PriceStatus::Trading,
				0,
				*expo,
				5,
				10_000_000,
				PriceIdentifier::new([0; 32]),
				*price,
				0,
				0,
				0,
				0,
				0,
				0
			)),
		}
	}

	/// This function currently is never used. 
	/// However it may make more sense to abstract out the usage of price_feeds with this,
	/// so that users of Basket only ever have to work with Pyth Price structs instead of messing with PriceFeeds
	pub fn get_price(&self, querier: &QuerierWrapper) -> Result<Price, ContractError> {
		match self {
			Self::Pyth{ addr, price_id } => {
				let price_feed = query_price_feed(querier, addr.to_string(), *price_id)?.price_feed;

				match price_feed.get_current_price() {
					Some(price) => Ok(price),
					None => Err(ContractError::OracleQueryFailed),
				}
			},

			Self::Stub{ price, expo } => Ok(Price {
				price: *price,
				conf: 0,
				expo: *expo
			}),
		}
	}
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

