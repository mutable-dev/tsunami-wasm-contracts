use crate::{
    error::ContractError,
    msg::*,
    asset::AssetInfo,
    state::{Basket, BasketAsset, BASKET},
};
use cosmwasm_bignumber::{Decimal256, Uint256};
use cosmwasm_std::{
    attr, entry_point, from_binary, to_binary, Addr, Binary, Coin, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Reply, ReplyOn, Response, StdError, StdResult, SubMsg, Uint128,
    WasmMsg, Api
};
use cw2::set_contract_version;
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg, MinterResponse};
use protobuf::Message;


/// Contract name that is used for migration.
const CONTRACT_NAME: &str = "tsunami-basket";
/// Contract version that is used for migration.
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const INSTANTIATE_BASKET_REPLY_ID: u64 = 1;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    
    // Check assets + Ensure no repeated assets
    check_assets(&msg.assets)?;

    // Set contract version
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // SubMsg to Create the LP token contract
    let token_name = format!("{}-LP", &msg.name);
    let sub_msg = instantiate_lp(&msg, env, token_name)?;

    // Build BasketAssets from message
    let assets: Vec<BasketAsset> = build_assets(&msg);

    // Build Basket from Assets and parameters in message
    let basket = Basket::new(assets, &msg);

    // Store Basket in Item/Singleton
    BASKET.save(deps.storage, &basket)?;

    // Return success with response
    Ok(Response::new().add_submessages(sub_msg))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::DepositLiquidity {} => Ok(Response::new()),
        ExecuteMsg::WithdrawLiquidity { cw20msg } => withdraw_liquidity(
            deps,
            env,
            info,
            Addr::unchecked(cw20msg.sender),
            cw20msg.amount,
            cw20msg.msg
        ),
    }
}

// #[cfg_attr(not(feature = "library"), entry_point)]
// pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
//     let mut basket: Basket = BASKET.load(deps.storage)?;

//     if basket.lp_token_address != Addr::unchecked("") {
//         return Err(ContractError::Unauthorized {});
//     }

//     let data = msg.result.unwrap().data.unwrap();
//     let res: MsgInstantiateContractResponse =
//         Message::parse_from_bytes(data.as_slice()).map_err(|_| {
//             StdError::parse_err("MsgInstantiateContractResponse", "failed to parse data")
//         })?;

//     config.pair_info.liquidity_token =
//         addr_validate_to_lower(deps.api, res.get_contract_address())?;

//     CONFIG.save(deps.storage, &config)?;

//     Ok(Response::new().add_attribute("liquidity_token_addr", config.pair_info.liquidity_token))
// }

pub fn withdraw_liquidity(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    sender: Addr,
    amount: Uint128,
    msg: Binary,
) -> Result<Response, ContractError> {
    // Load Basket
    let mut basket: Basket = BASKET.load(deps.storage).unwrap();

    // Abort if not from basket lp token contract
    if info.sender != basket.lp_token_address {
        return Err(ContractError::Unauthorized {});
    }

    // TODO: encode which asset to withdraw in msg (may not be possible. may need some reworking)
    // For now, just send back luna always 
    let asset_info = AssetInfo::NativeToken{ denom: "luna".to_string() }; // TODO; replace with some function of Binary msg
    let refund_asset = basket.withdraw_amount(amount, asset_info);//get_share_in_assets(&pools, amount, total_share);

    // Update the pool info
    let messages: Vec<CosmosMsg> = vec![
        refund_asset
            .clone()
            .into_msg(&deps.querier, sender.clone())?,
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: basket.lp_token_address.to_string(),
            msg: to_binary(&Cw20ExecuteMsg::Burn { amount })?,
            funds: vec![],
        }),
    ];

    let attributes = vec![
        attr("action", "withdraw_liquidity"),
        attr("sender", sender.as_str()),
        attr("withdrawn_share", &amount.to_string()),
        attr(
            "refund_asset",
            format!("{}", refund_asset),
        ),
    ];

    Ok(Response::new()
        .add_messages(messages)
        .add_attributes(attributes))
}

/// TODO: Need to implement this
fn validate_addr(
    api: &dyn Api,
    sender: &String
) -> Result<String, ContractError> {
    Ok(sender.clone())
}



fn instantiate_lp(
    msg: &InstantiateMsg,
    env: Env,
    token_name: String,
) -> Result<Vec<SubMsg>, ContractError> {
    Ok(vec![SubMsg {
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
            }).expect("failed to convert InstantiateLpMsg to binary."),
            funds: vec![],
            admin: None,
            label: "Tsunami LP Token".to_string(),
        }
        .into(),
        id: INSTANTIATE_BASKET_REPLY_ID,
        gas_limit: None,
        reply_on: ReplyOn::Success,
    }])
}

fn build_assets(
    msg: &InstantiateMsg,
) -> Vec<BasketAsset> {
    let mut assets = Vec::new();
    for asset in msg.assets.clone() {
        assets.push(BasketAsset::new(asset));
    }
    assets
}

fn check_assets(assets: &Vec<InstantiateAssetInfo>) -> Result<u64, ContractError>{
    let mut asset_names: Vec<String> = Vec::new();
    for asset in assets {
        if asset_names.contains(&asset.address.to_string()) {
            return Err(ContractError::DuplicateAssetAssertion{})
        }
        asset_names.push(asset.address.to_string());
    }
    Ok(1)
}

/// ## Description
/// Exposes all the queries available in the contract.
/// ## Params
/// * **deps** is an object of type [`Deps`].
///
/// * **_env** is an object of type [`Env`].
///
/// * **msg** is an object of type [`QueryMsg`].
///
/// ## Queries
/// * **QueryMsg::Basket {}** Returns information about the basket in an object of type [`Basket`].
#[cfg_attr(not(feature = "library"), entry_point)]
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Basket {} => to_binary(&query_basket(deps)?),
    }
}

/// ## Description
/// Returns information about the basket contract in an object of type [`BASKET`].
/// ## Params
/// * **deps** is an object of type [`Deps`].
pub fn query_basket(deps: Deps) -> StdResult<Basket> {
    BASKET.load(deps.storage)
}
