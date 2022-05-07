use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{fmt, convert::TryInto};

use cosmwasm_std::{
    to_binary, Addr, Api, BankMsg, Coin, CosmosMsg, Decimal, MessageInfo, QuerierWrapper, StdError,
    StdResult, Uint128, WasmMsg,
};
use cw20::Cw20ExecuteMsg;
use terra_cosmwasm::TerraQuerier;

use crate::{
    error::ContractError,
    state::BasketAsset,
    querier::query_token_precision, 
    price::PythPrice,
};

/// UST token denomination
pub const UUSD_DENOM: &str = "uusd";
/// LUNA token denomination
pub const ULUNA_DENOM: &str = "uluna";

const USD_VALUE_PRECISION: i32 = -6;

/// ## Description
/// This enum describes a Terra asset (native or CW20).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Asset {
    /// Information about an asset stored in a [`AssetInfo`] struct
    pub info: AssetInfo,
    /// A token amount
    pub amount: Uint128,
}

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{}", self.amount, self.info)
    }
}

/// Decimal points
static DECIMAL_FRACTION: Uint128 = Uint128::new(1_000_000_000_000_000_000u128);

impl Asset {
    /// Returns true if the token is native. Otherwise returns false.
    /// ## Params
    /// * **self** is the type of the caller object.
    pub fn is_native_token(&self) -> bool {
        self.info.is_native_token()
    }

    /// Calculates and returns a tax for a chain's native token. For other tokens it returns zero.
    /// ## Params
    /// * **self** is the type of the caller object.
    ///
    /// * **querier** is an object of type [`QuerierWrapper`]
    pub fn compute_tax(&self, querier: &QuerierWrapper) -> StdResult<Uint128> {
        let amount = self.amount;
        if let AssetInfo::NativeToken { denom } = &self.info {
            let terra_querier = TerraQuerier::new(querier);
            let tax_rate: Decimal = (terra_querier.query_tax_rate()?).rate;
            let tax_cap: Uint128 = (terra_querier.query_tax_cap(denom.to_string())?).cap;
            Ok(std::cmp::min(
                (amount.checked_sub(amount.multiply_ratio(
                    DECIMAL_FRACTION,
                    DECIMAL_FRACTION * tax_rate + DECIMAL_FRACTION,
                )))?,
                tax_cap,
            ))
        } else {
            Ok(Uint128::zero())
        }
    }

    /// Calculates and returns a deducted tax for transferring the native token from the chain. For other tokens it returns an [`Err`].
    /// ## Params
    /// * **self** is the type of the caller object.
    ///
    /// * **querier** is an object of type [`QuerierWrapper`]
    pub fn deduct_tax(&self, querier: &QuerierWrapper) -> StdResult<Coin> {
        let amount = self.amount;
        if let AssetInfo::NativeToken { denom } = &self.info {
            Ok(Coin {
                denom: denom.to_string(),
                amount: amount.checked_sub(self.compute_tax(querier)?)?,
            })
        } else {
            Err(StdError::generic_err("cannot deduct tax from token asset"))
        }
    }

    /// Returns a message of type [`CosmosMsg`].
    ///
    /// For native tokens of type [`AssetInfo`] uses the default method [`BankMsg::Send`] to send a token amount to a recipient.
    /// Before the token is sent, we need to deduct a tax.
    ///
    /// For a token of type [`AssetInfo`] we use the default method [`Cw20ExecuteMsg::Transfer`] and so there's no need to deduct any other tax.
    /// ## Params
    /// * **self** is the type of the caller object.
    ///
    /// * **querier** is an object of type [`QuerierWrapper`]
    ///
    /// * **recipient** is the address where the funds will be sent.
    #[allow(clippy::redundant_field_names)]
    pub fn into_msg(self, _querier: &QuerierWrapper, recipient: Addr) -> StdResult<CosmosMsg> {
        let amount = self.amount;

        match &self.info {
            AssetInfo::Token { contract_addr } => Ok(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: contract_addr.to_string(),
                msg: to_binary(&Cw20ExecuteMsg::Transfer {
                    recipient: recipient.to_string(),
                    amount,
                })?,
                funds: vec![],
            })),
            AssetInfo::NativeToken { denom } => Ok(CosmosMsg::Bank(BankMsg::Send {
                to_address: recipient.to_string(),
                amount: vec![Coin {
                    denom: denom.to_string(),
                    amount: amount,
                }],
            })),
        }
    }

    /// Validates an amount of native tokens being sent. Returns [`Ok`] if successful, otherwise returns [`Err`].
    /// ## Params
    /// * **self** is the type of the caller object.
    ///
    /// * **message_info** is an object of type [`MessageInfo`]
    pub fn assert_sent_native_token_balance(&self, message_info: &MessageInfo) -> StdResult<()> {
        if let AssetInfo::NativeToken { denom } = &self.info {
            match message_info.funds.iter().find(|x| x.denom == *denom) {
                Some(coin) => {
                    if self.amount == coin.amount {
                        Ok(())
                    } else {
                        Err(StdError::generic_err("Native token balance mismatch between the argument and the transferred"))
                    }
                }
                None => {
                    if self.amount.is_zero() {
                        Ok(())
                    } else {
                        Err(StdError::generic_err("Native token balance mismatch between the argument and the transferred"))
                    }
                }
            }
        } else {
            Ok(())
        }
    }
}

/// This enum describes available Token types.
/// ## Examples
/// ``` ignore
/// # use cosmwasm_std::Addr;
/// # use astroport::asset::AssetInfo::{NativeToken, Token};
/// Token { contract_addr: Addr::unchecked("terra...") };
/// NativeToken { denom: String::from("uluna") };
/// ```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetInfo {
    /// Non-native Token
    Token { contract_addr: Addr },
    /// Native token
    NativeToken { denom: String },
}

impl fmt::Display for AssetInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AssetInfo::NativeToken { denom } => write!(f, "{}", denom),
            AssetInfo::Token { contract_addr } => write!(f, "{}", contract_addr),
        }
    }
}

impl AssetInfo {
    /// Returns true if the caller is a native token. Otherwise returns false.
    /// ## Params
    /// * **self** is the caller object type
    pub fn is_native_token(&self) -> bool {
        match self {
            AssetInfo::NativeToken { .. } => true,
            AssetInfo::Token { .. } => false,
        }
    }
    /// Returns True if the calling token is the same as the token specified in the input parameters.
    /// Otherwise returns False.
    /// ## Params
    /// * **self** is the type of the caller object.
    ///
    /// * **asset** is object of type [`AssetInfo`].
    pub fn equal(&self, asset: &AssetInfo) -> bool {
        match self {
            AssetInfo::Token { contract_addr, .. } => {
                let self_contract_addr = contract_addr;
                match asset {
                    AssetInfo::Token { contract_addr, .. } => self_contract_addr == contract_addr,
                    AssetInfo::NativeToken { .. } => false,
                }
            }
            AssetInfo::NativeToken { denom, .. } => {
                let self_denom = denom;
                match asset {
                    AssetInfo::Token { .. } => false,
                    AssetInfo::NativeToken { denom, .. } => self_denom == denom,
                }
            }
        }
    }

    /// If the caller object is a native token of type ['AssetInfo`] then his `denom` field converts to a byte string.
    ///
    /// If the caller object is a token of type ['AssetInfo`] then his `contract_addr` field converts to a byte string.
    /// ## Params
    /// * **self** is the type of the caller object.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            AssetInfo::NativeToken { denom } => denom.as_bytes(),
            AssetInfo::Token { contract_addr } => contract_addr.as_bytes(),
        }
    }

    /// Returns [`Ok`] if the token of type [`AssetInfo`] is in lowercase and valid. Otherwise returns [`Err`].
    /// ## Params
    /// * **self** is the type of the caller object.
    ///
    /// * **api** is a object of type [`Api`]
    pub fn check(&self, api: &dyn Api) -> StdResult<()> {
        match self {
            AssetInfo::Token { contract_addr } => {
                addr_validate_to_lower(api, contract_addr.as_str())?;
            }
            AssetInfo::NativeToken { denom } => {
                if !denom.starts_with("ibc/") && denom != &denom.to_lowercase() {
                    return Err(StdError::generic_err(format!(
                        "Non-IBC token denom {} should be lowercase",
                        denom
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Returns a lowercased, validated address upon success. Otherwise returns [`Err`]
/// ## Params
/// * **api** is an object of type [`Api`]
///
/// * **addr** is an object of type [`Addr`]
pub fn addr_validate_to_lower(api: &dyn Api, addr: &str) -> StdResult<Addr> {
    if addr.to_lowercase() != addr {
        return Err(StdError::generic_err(format!(
            "Address {} should be lowercase",
            addr
        )));
    }
    api.addr_validate(addr)
}

/// Returns an [`Asset`] object representing a native token and an amount of tokens.
/// ## Params
/// * **denom** is a [`String`] that represents the native asset denomination.
///
/// * **amount** is a [`Uint128`] representing an amount of native assets.
pub fn native_asset(denom: String, amount: Uint128) -> Asset {
    Asset {
        info: AssetInfo::NativeToken { denom },
        amount,
    }
}

/// Returns an [`Asset`] object representing a non-native token and an amount of tokens.
/// ## Params
/// * **contract_addr** is a [`Addr`]. It is the address of the token contract.
///
/// * **amount** is a [`Uint128`] representing an amount of tokens.
pub fn token_asset(contract_addr: Addr, amount: Uint128) -> Asset {
    Asset {
        info: AssetInfo::Token { contract_addr },
        amount,
    }
}

/// Returns an [`AssetInfo`] object representing the denomination for a Terra native asset.
/// ## Params
/// * **denom** is a [`String`] object representing the denomination of the Terra native asset.
pub fn native_asset_info(denom: String) -> AssetInfo {
    AssetInfo::NativeToken { denom }
}

/// Returns an [`AssetInfo`] object representing the address of a token contract.
/// ## Params
/// * **contract_addr** is a [`Addr`] object representing the address of a token contract.
pub fn token_asset_info(contract_addr: Addr) -> AssetInfo {
    AssetInfo::Token { contract_addr }
}

// PricedAsset
#[derive(Debug)]
pub struct PricedAsset {
    pub asset: Asset,
    pub basket_asset: BasketAsset,
    decimals: Option<i32>,
    price: Option<PythPrice>,
}

impl PricedAsset {
    pub fn new(asset: Asset, basket_asset: BasketAsset) -> Self {
        PricedAsset { asset, basket_asset, price: None, decimals: None }
    }

    pub fn query_decimals(&mut self, querier: &QuerierWrapper) -> Result<i32, ContractError> {
        let decimals: i32 = query_token_precision(querier, &self.asset.info)?
            .try_into()
            .expect("Unable to query for offer token decimals");
        self.decimals = Some(decimals);
        Ok(decimals)
    }

    pub fn query_price(&mut self, querier: &QuerierWrapper) -> Result<PythPrice, ContractError> {
        match self.price {
            Some(price) => Ok(price),
            None => {
                let price = PythPrice::new(self.basket_asset.oracle.get_price(querier)?);
                self.price = Some(price);
                Ok(price)
            }
        }
    }

    pub fn query_contract_value(&mut self, querier: &QuerierWrapper) -> Result<Uint128, ContractError> {
        let decimals = self.query_decimals(querier)?;
        let price: PythPrice = self.query_price(querier)?;
        let value = if price.pyth_price.expo < 0 {
            Uint128::from(price.pyth_price.price as u128)
            .multiply_ratio(
                (self.basket_asset.available_reserves.u128()
                + self.basket_asset.occupied_reserves.u128()) * 10_u128.pow(-USD_VALUE_PRECISION as u32),
                10_u128.pow(price.pyth_price.expo.unsigned_abs() + decimals.unsigned_abs())
            )
        } else {
            Uint128::from(price.pyth_price.price as u128)
            .multiply_ratio(
                (self.basket_asset.available_reserves.u128()
                + self.basket_asset.occupied_reserves.u128()) * 10_u128.pow(-USD_VALUE_PRECISION as u32 + price.pyth_price.expo.unsigned_abs()),
                10_u128.pow(decimals as u32)
            )
        };
        Ok(value)
    }

    pub fn query_value(&mut self, querier: &QuerierWrapper) -> Result<Uint128, ContractError> {
        let decimals = self.query_decimals(querier)?;
        let price: PythPrice = self.query_price(querier)?;
        let value = if price.pyth_price.expo < 0 {
            Uint128::from(price.pyth_price.price as u128)
            .multiply_ratio(
                self.asset.amount.u128() * 10_u128.pow(-USD_VALUE_PRECISION as u32),
                10_u128.pow(price.pyth_price.expo.unsigned_abs() + decimals.unsigned_abs())
            )
        } else {
            Uint128::from(price.pyth_price.price as u128)
            .multiply_ratio(
                self.asset.amount.u128() * 10_u128.pow(-USD_VALUE_PRECISION as u32 + price.pyth_price.expo.unsigned_abs()),
                10_u128.pow(decimals as u32)
            )
        };
        Ok(value)
    }
}

pub fn safe_u128_to_i64(input: u128) -> Result<i64, ContractError> {
    let output = input as i64;
    if output as u128 == input {
        Ok(output)
    } else {
        Err(ContractError::FailedCast)
    }
}

pub fn safe_i64_to_u128(input: i64) -> Result<u128, ContractError> {
    let output = input as u128;
    if output as i64 == input {
        Ok(output)
    } else {
        Err(ContractError::FailedCast)
    }
}
