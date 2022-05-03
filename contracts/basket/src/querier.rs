use crate::asset::AssetInfo;
use crate::error::ContractError;
use cosmwasm_std::{to_binary, Addr, QuerierWrapper, QueryRequest, Uint128, WasmQuery};

use cw20::{Cw20QueryMsg, TokenInfoResponse};

// It's defined at https://github.com/terra-money/core/blob/d8e277626e74f9d6417dcd598574686882f0274c/types/assets/assets.go#L15
const NATIVE_TOKEN_PRECISION: u8 = 6;

/// Returns the total supply of a specific token.
/// ## Params
/// * **querier** is an object of type [`QuerierWrapper`].
///
/// * **contract_addr** is an object of type [`Addr`] which is the token contract address.
pub fn query_supply(querier: &QuerierWrapper, contract_addr: Addr) -> Result<Uint128, ContractError> {
    let res: TokenInfoResponse = querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: String::from(contract_addr),
        msg: to_binary(&Cw20QueryMsg::TokenInfo {}).map_err(|_| ContractError::FailedToQueryTokenSupply)?,
    }))?;

    Ok(res.total_supply)
}

/// Returns the number of decimals that a token has.
/// ## Params
/// * **querier** is an object of type [`QuerierWrapper`].
///
/// * **asset_info** is an object of type [`AssetInfo`] and contains the asset details for a specific token.
pub fn query_token_precision(querier: &QuerierWrapper, asset_info: &AssetInfo) -> Result<u8, ContractError> {
    Ok(match asset_info {
        AssetInfo::NativeToken { denom: _ } => NATIVE_TOKEN_PRECISION,
        AssetInfo::Token { contract_addr } => {
            let res: TokenInfoResponse =
                querier.query_wasm_smart(contract_addr, &Cw20QueryMsg::TokenInfo {}).map_err(|_| ContractError::FailedToQueryTokenDecimals)?;

            res.decimals
        }
    })
}
