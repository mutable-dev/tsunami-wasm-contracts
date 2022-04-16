use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Uint128, QuerierWrapper, Uint256};
use std::convert::{TryFrom, TryInto};
use cw_storage_plus::{Item, Map};
use crate::error::ContractError;
use crate::asset::{Asset, AssetInfo};
use crate::msg::{InstantiateMsg, InstantiateAssetInfo};
use pyth_sdk_terra::{PriceFeed, Price, PriceIdentifier, PriceStatus, query_price_feed};
use std::cfg;

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
	/// cache the total weights of the assets	
	pub total_weights: Uint128,
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
			total_weights: msg.total_weights,
			admin: msg.admin.clone(),
			lp_token_address: Addr::unchecked(""), // This is fixed in reply
		}
	}

	// /// TODO: Calculates AUM
	// pub fn calculate_aum(&self) -> Uint128 {
	// 	Uint128::from(1_u32)
	// }

	// CHECK: that we should take the value of the token account as AUM and not the general reserves from the
	// available asset account
	pub fn calculate_aum(
		&self,
		prices: &[PriceFeed], 
		reserve_basket_asset_info: &AssetInfo,
	) -> Result<AumResult, ContractError> {
		let mut aum = Uint128::new(0);
		let mut precise_price = 0;
		let mut exponent =  1;
		let mut current_basket_asset: &BasketAsset = &self.assets[0];
		let reserve_asset_denom: String;
		match reserve_basket_asset_info {
				AssetInfo::NativeToken{ denom } => reserve_asset_denom = denom.to_string(),
				_ => {
						return Err(ContractError::NonNativeAssetAssertion);
				}
		}

		for (i, pyth_price) in prices.iter().enumerate() {
			current_basket_asset = &self.assets[i];

			let price_option = pyth_price.get_current_price();
					let price: Price;
					match price_option {
							Some(price_res) => price = price_res,
							_ => return Err(ContractError::OracleQueryFailed)
					};

					// Assumes only native assets for now
					let current_asset_info: &AssetInfo = &current_basket_asset.info; 

					match current_asset_info {
							AssetInfo::NativeToken{ denom } => {
									if denom == &reserve_asset_denom {
											exponent = price.expo.abs();
											precise_price = price.price;
									}
							},
							_ =>  ()
					}

			aum += Uint128::try_from(
				Uint256::from_uint128(current_basket_asset.pool_reserves)
				.checked_mul(Uint256::from_uint128(Uint128::new(price.price as u128)))
				.unwrap()
				.checked_div(
					Uint256::from_uint128(Uint128::new(10_u64.pow(price.expo.abs() as u32) as u128))
				)
				.unwrap())
				.unwrap();
		}
		Ok(AumResult{ aum, price: precise_price, exponent })
	}
	

	/// TODO: Calculates total number of lp tokens
	pub fn total_tokens(&self) -> Uint128 {
		Uint128::from(1_u32)
	}

	/// TODO: Calculates amount to withdraw. Reduce fees elsewhere
	pub fn withdraw_amount(&self, lp_amount: Uint128, prices: &[PriceFeed], info: AssetInfo, ) -> Result<Asset, ContractError> {
		let aum_result = self.calculate_aum(prices, &info);
		match aum_result {
			Ok(res) => Ok(Asset {
				info: info,
				amount: (lp_amount * res.aum) / self.total_tokens()
			}),
			Err(err) => Err(err)
		}
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
