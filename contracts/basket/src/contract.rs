use crate::{
    error::ContractError,
    msg::*,
    state::{Basket, Asset, BASKET},
};
use cosmwasm_bignumber::{Decimal256, Uint256};
use cosmwasm_std::{
    attr, entry_point, from_binary, to_binary, Addr, Binary, Coin, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Reply, ReplyOn, Response, StdError, StdResult, SubMsg, Uint128,
    WasmMsg,
};
use cw2::set_contract_version;
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg, MinterResponse};


/// Contract name that is used for migration.
const CONTRACT_NAME: &str = "tsunami-basket";
/// Contract version that is used for migration.
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const INSTANTIATE_BUCKET_REPLY_ID: u64 = 1;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    
    // Check assets + Ensure no repeated assets
    check_assets();

    // Set contract version
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let mut assets = Vec::<Asset>::new();
    for asset in msg.assets {
        assets.push(Asset::new(asset));
    }

    let basket = Basket {
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
	    admin: msg.admin,
    };

    BASKET.save(deps.storage, &basket)?;

    
    let token_name = format!("{}-LP", &msg.name);

    // create LP token
    // Create the LP token contract
    let sub_msg: Vec<SubMsg> = vec![SubMsg {
        msg: WasmMsg::Instantiate {
            code_id: msg.token_code_id,
            msg: to_binary(&InstantiateLpMsg {
                name: token_name,
                symbol: "NLP".to_string(),
                decimals: 6,
                initial_balances: vec![],
                mint: Some(MinterResponse {
                    minter: env.contract.address.to_string(),
                    cap: None,
                }),
            })?,
            funds: vec![],
            admin: None,
            label: String::from("Tsunami LP Token"),
        }
        .into(),
        id: INSTANTIATE_BUCKET_REPLY_ID,
        gas_limit: None,
        reply_on: ReplyOn::Success,
    }];
    
    Ok(Response::new().add_submessages(sub_msg))
}



fn check_assets() {}