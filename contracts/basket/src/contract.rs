use crate::{
    error::ContractError,
    msg::*,
    asset::{AssetInfo, Asset, addr_validate_to_lower},
    state::{Basket, BasketAsset, BASKET, ToAssetInfo},
    querier::{query_supply, query_token_precision},
};
use cosmwasm_std::{
    attr, entry_point, from_binary, to_binary, Addr, Binary, Coin, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Reply, ReplyOn, Response, StdError, StdResult, SubMsg, Uint128,
    WasmMsg, Api, Uint256
};
use cw2::set_contract_version;
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg, MinterResponse};
use std::cmp::max;
use pyth_sdk_terra::{PriceFeed, Price, PriceIdentifier, PriceStatus};
use std::convert::{TryInto, TryFrom};
use protobuf::Message;
use itertools::izip;


/// Contract name that is used for migration.
const CONTRACT_NAME: &str = "tsunami-basket";
/// Contract version that is used for migration.
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const INSTANTIATE_BASKET_REPLY_ID: u64 = 1;
const BASIS_POINTS_PRECISION: Uint128 = Uint128::new(10_000);
const BASE_FEE_IN_BASIS_POINTS: Uint128 = Uint128::new(15);
const PENALTY_IN_BASIS_POINTS: Uint128 = Uint128::new(15);

// Calculate USD value of asset down to this precision
pub const USD_VALUE_PRECISION: i32 = -6;
pub const LP_DECIMALS: u8 = 9;


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
        ExecuteMsg::DepositLiquidity {assets, slippage_tolerance, receiver} => provide_liquidity(deps, env, info, assets, slippage_tolerance, receiver),
        ExecuteMsg::Receive { msg } => receive_cw20(deps, env, info, msg),
        ExecuteMsg::Swap { sender, offer_asset, belief_price, max_spread, to, ask_asset } => swap(deps, env, info, sender, offer_asset, belief_price, max_spread, to, ask_asset),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    let mut basket: Basket = BASKET.load(deps.storage)?;

    if basket.lp_token_address != Addr::unchecked("") {
        return Err(ContractError::Unauthorized);
    }

    let data = msg.result.unwrap().data.unwrap();
    let res: MsgInstantiateContractResponse =
        Message::parse_from_bytes(data.as_slice()).map_err(|_| {
            StdError::parse_err("MsgInstantiateContractResponse", "failed to parse data")
        })?;

    basket.lp_token_address =
        addr_validate_to_lower(deps.api, res.get_contract_address())?;

    BASKET.save(deps.storage, &basket)?;

    Ok(Response::new().add_attribute("liquidity_token_addr", basket.lp_token_address))
}

pub fn withdraw_liquidity(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    sender: Addr,
    amount: Uint128,
    ask_asset: BasketAsset,
) -> Result<Response, ContractError> {
    
    // Load Basket
    let basket: Basket = BASKET.load(deps.storage).unwrap();

    // Abort if not from basket lp token contract
    if info.sender != basket.lp_token_address {
        return Err(ContractError::Unauthorized);
    }

    // Retrieve ask asset
    let assets = basket.assets.clone();
    let ask_decimals = query_token_precision(&deps.querier, &ask_asset.info)? as i32;
    let ask_asset_with_price: (BasketAsset, Price) = match assets.iter().zip(basket.get_prices(&deps.querier)?)
        .find(|(asset, _price)| ask_asset.info.equal(&asset.info)) {
            Some((asset, price)) => (asset.clone(), price.clone()),
            None => return Err(ContractError::AssetNotInBasket)
    };
    // Determine the amount of an asset held in the contract based on our internal accounting
    let ask_asset_value_in_contract: Uint128 = safe_price_to_Uint128(
        Price::price_basket(
            &[(
                ask_asset_with_price.1,
                safe_u128_to_i64(ask_asset_with_price.0.available_reserves.u128())? +
                safe_u128_to_i64(ask_asset_with_price.0.occupied_reserves.u128())?, 
                -ask_decimals
            )],
            USD_VALUE_PRECISION
        ).expect("couldn't price ask asset")
    );


    // Calculate gross asset return
    let mut redemption_value: Uint128 = basket.withdraw_amount(amount, ask_asset.info.clone(), &deps.querier)?;


    // TODO: Calculate fee_bps
    let initial_aum_value: Uint128 = safe_price_to_Uint128(basket.calculate_aum(&deps.querier)?);
    let fee_bps: Uint128 = calculate_fee_basis_points(
        initial_aum_value, 
        &basket, 
        &vec![ask_asset_value_in_contract], 
        &vec![redemption_value],
        &vec![ask_asset.clone()],
        Action::Ask
    )[0];

    // Update refund_asset with fee
    redemption_value = redemption_value.multiply_ratio(BASIS_POINTS_PRECISION - fee_bps, BASIS_POINTS_PRECISION);
    // milli-USDs per token
    let invert_price: Price = get_unit_price().div(&ask_asset_with_price.1).unwrap();
    let refund_amount = redemption_value / safe_price_to_Uint128(invert_price);
    let refund_asset = Asset {
        amount: refund_amount,
        info: ask_asset.info.clone()
    };

    // Update the asset info
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
) -> Result<Addr, ContractError> {
    Ok(Addr::unchecked(sender.clone()))
}


/// Produces unit price of USD, in units of `USD_VALUE_PRECISION`
pub fn get_unit_price() -> Price {
    Price {
        price: 1,//10_i64.pow(-USD_VALUE_PRECISION as u32),
        expo: USD_VALUE_PRECISION,
        conf: 0
    }
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
                // TODO: replace with some generator, or perhaps with some admin-specified value in instantiate msg
                symbol: "TLP".to_string(),
                decimals: LP_DECIMALS,
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
            return Err(ContractError::DuplicateAssetAssertion)
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



/// ## Description
/// Receives a message of type [`Cw20ReceiveMsg`] and processes it depending on the received template.
/// If the template is not found in the received message, then an [`ContractError`] is returned,
/// otherwise it returns the [`Response`] with the specified attributes if the operation was successful.
/// ## Params
/// * **deps** is an object of type [`DepsMut`].
///
/// * **env** is an object of type [`Env`].
///
/// * **info** is an object of type [`MessageInfo`].
///
/// * **cw20_msg** is an object of type [`Cw20ReceiveMsg`]. This is the CW20 message that has to be processed.
pub fn receive_cw20(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cw20_msg: Cw20ReceiveMsg,
) -> Result<Response, ContractError> {
    let contract_addr = info.sender.clone();
    match from_binary(&cw20_msg.msg) {
        Ok(Cw20HookMsg::Swap {
            belief_price,
            max_spread,
            to,
            ask_asset,
        }) => {

            // Only asset contract can execute this message
            let mut authorized: bool = false;
            let basket = BASKET.load(deps.storage)?;

            for asset in basket.assets {
                if let AssetInfo::Token { contract_addr, .. } = &asset.info {
                    if contract_addr == &info.sender {
                        authorized = true;
                    }
                }
            }

            if !authorized {
                return Err(ContractError::Unauthorized);
            }

            let to_addr = if let Some(to_addr) = to {
                Some(validate_addr(deps.api, &to_addr)?)
            } else {
                None
            };

            swap(
                deps,
                env,
                info,
                Addr::unchecked(cw20_msg.sender),
                Asset {
                    info: AssetInfo::Token { contract_addr },
                    amount: cw20_msg.amount,
                },
                belief_price,
                max_spread,
                to_addr,
                ask_asset,
            )
        }
        Ok(Cw20HookMsg::WithdrawLiquidity { basket_asset} ) => withdraw_liquidity(
            deps,
            env,
            info,
            Addr::unchecked(cw20_msg.sender),
            cw20_msg.amount,
            basket_asset
        ),
        Err(err) => Err(ContractError::Std(err)),
    }
}





/// ## Description
/// Performs an swap operation with the specified parameters. The trader must approve the
/// pool contract to transfer offer assets from their wallet.
/// Returns an [`ContractError`] on failure, otherwise returns the [`Response`] with the specified attributes if the operation was successful.
/// ## Params
/// * **deps** is an object of type [`DepsMut`].
///
/// * **env** is an object of type [`Env`].
///
/// * **info** is an object of type [`MessageInfo`].
///
/// * **sender** is an object of type [`Addr`]. This is the sender of the swap operation.
///
/// * **offer_asset** is an object of type [`Asset`]. Proposed asset for swapping.
///
/// * **belief_price** is an object of type [`Option<Decimal>`]. Used to calculate the maximum swap spread.
///
/// * **max_spread** is an object of type [`Option<Decimal>`]. Sets the maximum spread of the swap operation.
///
/// * **to** is an object of type [`Option<Addr>`]. Sets the recipient of the swap operation.
/// NOTE - the address that wants to swap should approve the pair contract to pull the offer token.
#[allow(clippy::too_many_arguments)]
pub fn swap(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    sender: Addr,
    offer_asset: Asset,
    belief_price: Option<Decimal>,
    max_spread: Option<Decimal>,
    to: Option<Addr>,
    ask_asset: AssetInfo,
) -> Result<Response, ContractError> {

    // Ensure native token was sent
    offer_asset.assert_sent_native_token_balance(&info)?;

    // Load basket singleton, get assets
    let mut basket: Basket = BASKET.load(deps.storage)?;

    let mut messages: Vec<CosmosMsg> = vec![];
    for (i, asset) in basket.assets.iter_mut().enumerate() {
        // If the asset is a token contract, then we need to execute a TransferFrom msg to receive assets
        if let AssetInfo::Token { contract_addr, .. } = &asset.info {
            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: contract_addr.to_string(),
                msg: to_binary(&Cw20ExecuteMsg::TransferFrom {
                    owner: info.sender.to_string(),
                    recipient: env.contract.address.to_string(),
                    amount: offer_asset.amount,
                })?,
                funds: vec![],
            }));
        }
    }

    // Grab relevant asset assets in basket, zipped with price
    let offer_decimals: i32 = query_token_precision(&deps.querier, &offer_asset.info)?.try_into().unwrap();
    let offer_asset_with_price: (BasketAsset, Price, Asset) = match basket.assets.iter()
        .zip(basket.get_prices(&deps.querier)?)
        .find(|(asset, _price)| offer_asset.info.equal(&asset.info)) {
            Some((asset, price)) => ( asset.clone(),  price.clone(), offer_asset.clone()),
            None => return Err(ContractError::AssetNotInBasket)
    };
    // Determine the amount of an asset held in the contract based on our internal accounting
    let offer_asset_value_in_contract: Uint128 = safe_price_to_Uint128(
        Price::price_basket(
            &[(
                offer_asset_with_price.1, 
                safe_u128_to_i64(offer_asset_with_price.0.available_reserves.u128())? +
                safe_u128_to_i64(offer_asset_with_price.0.occupied_reserves.u128())?, 
                -offer_decimals
            )],
            USD_VALUE_PRECISION
        ).unwrap()
    );
    let ask_decimals: i32 = query_token_precision(&deps.querier, &ask_asset)?.try_into().unwrap();
    let ask_asset_with_price: (BasketAsset, Price) = match basket.assets.iter().zip(basket.get_prices(&deps.querier)?)
        .find(|(asset, _price)| ask_asset.equal(&asset.info)) {
            Some((asset, price)) => (asset.clone(), price.clone()),
            None => return Err(ContractError::AssetNotInBasket)
    };
    
    // Determine the amount of an asset held in the contract based on our internal accounting
    let ask_asset_value_in_contract: Uint128 = safe_price_to_Uint128(
        Price::price_basket(
            &[(
                ask_asset_with_price.1, 
                safe_u128_to_i64(ask_asset_with_price.0.available_reserves.u128())? +
                safe_u128_to_i64(ask_asset_with_price.0.occupied_reserves.u128())?, 
                -ask_decimals
            )],
            USD_VALUE_PRECISION
        ).unwrap()
    );

    // TODO: Compute offer value and ask fee 
    // let initial_aum_value: Uint128 = safe_price_to_Uint128(basket.calculate_aum(&deps.querier)?);
    let initial_aum_value = Uint128::new(basket.calculate_aum(&deps.querier)?.price as u128);
    let price_basket = Price::price_basket(&[(
        offer_asset_with_price.1, 
        safe_u128_to_i64(offer_asset.amount.u128()).unwrap(),
        -offer_decimals
    )], USD_VALUE_PRECISION).unwrap();
    let user_offer_value = Uint128::new(price_basket.price as u128);
    let offer_fee_bps: Uint128 = calculate_fee_basis_points(
        initial_aum_value, 
        &basket, 
        &vec![offer_asset_value_in_contract], 
        &vec![user_offer_value],
        &basket.match_basket_assets(&vec![offer_asset.info.clone()]),
        Action::Offer
    )[0];
    let ask_fee_bps: Uint128 = calculate_fee_basis_points(
        initial_aum_value, 
        &basket, 
        &vec![ask_asset_value_in_contract], 
        &vec![user_offer_value],
        &basket.match_basket_assets(&vec![ask_asset.clone()]),
        Action::Ask
    )[0];


    // Calculate post-fee USD value, then convert USD value to number of tokens.
    let refund_value = user_offer_value
        .multiply_ratio(
            BASIS_POINTS_PRECISION - ask_fee_bps - offer_fee_bps,
            BASIS_POINTS_PRECISION
    );
    // Get value of ask per unit usd, e.g. microUSD
    let ask_per_unit_usd = ask_asset_with_price.1.price as u128;
    // The price of a lamport is 10^ask_decimals lower, so multiply refund_value by appropriate power of 10 then divide by ask price
    let refund_amount = refund_value.multiply_ratio(10_u128.pow(ask_decimals as u32), ask_per_unit_usd);

    // Construct asset type and convert to message to `to` or `sender`
    let return_asset = Asset {
        info: ask_asset_with_price.0.info.clone(),
        amount: refund_amount,
    };
    let receiver = to.unwrap_or_else(|| sender.clone());
    let messages: Vec<CosmosMsg> =
        vec![return_asset.into_msg(&deps.querier, receiver.clone())?];

    match basket.assets.iter_mut().find(|asset| offer_asset.info.equal(&asset.info)) {
        Some(offer_basket_asset) => { offer_basket_asset.available_reserves += offer_asset.amount },
        None => {}
    }

    match basket.assets.iter_mut().find(|asset| ask_asset.equal(&asset.info)) {
        Some(offer_asset) => { offer_asset.available_reserves -= refund_amount },
        None => {}
    }

    // Save state
    BASKET.save(deps.storage, &basket)?;

    // 
    Ok(Response::new()
        .add_messages(
            // 1. send collateral tokens from the contract to a user
            // 2. send inactive commission fees to the Maker contract
            messages,
        )
        .add_attribute("action", "swap")
        .add_attribute("sender", sender.as_str())
        .add_attribute("receiver", receiver.as_str())
        .add_attribute("offer_asset", offer_asset.info.to_string())
        .add_attribute("ask_asset", ask_asset_with_price.0.info.to_string())
        .add_attribute("offer_amount", offer_asset.amount.to_string())
        .add_attribute("return_amount", refund_amount.to_string())
        .add_attribute("offer_bps", offer_fee_bps.to_string())
        .add_attribute("ask_bps", ask_fee_bps.to_string())
    )
}

// cases to consider
// 1. initialAmount is far from targetAmount, action increases balance slightly => high rebate.
// 2. initialAmount is far from targetAmount, action increases balance largely => high rebate.
// 3. initialAmount is close to targetAmount, action increases balance slightly => low rebate.
// 4. initialAmount is far from targetAmount, action reduces balance slightly => high tax.
// 5. initialAmount is far from targetAmount, action reduces balance largely => high tax.
// 6. initialAmount is close to targetAmount, action reduces balance largely => low tax.
// 7. initialAmount is above targetAmount, nextAmount is below targetAmount and vice versa.
// 8. a large swap should have similar fees as the same trade split into multiple smaller swaps.
/// CHECK: types here are bad, and conversions too many, need to consolidate.
/// CHECK: that we are doing the correct math when calculating
/// fees that should be charged .
/// CHECK: that we are calculating available assets correctly.
/// CHECK: that we should calculate the current reserves to compare against target reserves using 
/// only the available asset, relies on how AUM is calculated.
/// 
/// This returns.
pub fn calculate_fee_basis_points(
    initial_aum_value: Uint128,
    basket: &Basket,
	initial_reserve_values: &Vec<Uint128>,
	offer_or_ask_values: &Vec<Uint128>,
    offer_or_ask_assets: &Vec<BasketAsset>,
	action: Action
) -> Vec<Uint128> {
    
    // Compute new aum_value
    let new_aum_value: Uint128 = initial_aum_value + offer_or_ask_values.iter().sum::<Uint128>();

    // Compute updated reserve value by adding or subtracting diff_usd_value based on action
    let next_reserve_usd_values: Vec<Uint128> = match action {
        Action::Offer => initial_reserve_values.iter().zip(offer_or_ask_values).map(|(&a, &b)| a + b).collect(),
        Action::Ask => initial_reserve_values.iter().zip(offer_or_ask_values).map(|(&a, &b)| a.checked_sub(b).expect("ask too large")).collect(),
    };
	
    let mut fee_bps: Vec<Uint128> = vec![];
    for i in 0..offer_or_ask_assets.len() {

        let offer_or_ask_asset = offer_or_ask_assets[i].clone();
        let initial_reserve_value = initial_reserve_values[i].clone();
        let next_reserve_usd_value = next_reserve_usd_values[i].clone();

        // Compute target value based on weight, so that we may compare to the updated value
        let initial_target_lp_usd_value: Uint128 = initial_aum_value
            .multiply_ratio(offer_or_ask_asset.token_weight, basket.get_total_weights());
        let new_target_lp_usd_value: Uint128 = new_aum_value
            .multiply_ratio(offer_or_ask_asset.token_weight, basket.get_total_weights());

        // First depositor should not be hit with a fee
        if new_aum_value.is_zero() || initial_reserve_value.is_zero(){
            fee_bps.push(Uint128::zero());
        }

        // Calculate the initial and new distance from the target value
        let initial_distance: Uint128 = initial_target_lp_usd_value.max(initial_reserve_value) - initial_target_lp_usd_value.min(initial_reserve_value);
        let new_distance: Uint128 = new_target_lp_usd_value.max(next_reserve_usd_value) - new_target_lp_usd_value.min(next_reserve_usd_value);
        let improvement = new_distance <= initial_distance;

        if improvement {
            fee_bps.push(BASE_FEE_IN_BASIS_POINTS.multiply_ratio(initial_target_lp_usd_value - initial_distance.min(new_target_lp_usd_value), initial_target_lp_usd_value));
        } else {
            fee_bps.push(BASE_FEE_IN_BASIS_POINTS + PENALTY_IN_BASIS_POINTS.multiply_ratio(new_distance.min(new_target_lp_usd_value), new_target_lp_usd_value));
        }
    }
    fee_bps
}

pub enum Action {
    Offer,
    Ask
}

/// ## Description
/// Provides liquidity in the pair with the specified input parameters.
/// Returns a [`ContractError`] on failure, otherwise returns a [`Response`] with the specified
/// attributes if the operation was successful.
/// ## Params
/// * **deps** is an object of type [`DepsMut`].
///
/// * **env** is an object of type [`Env`].
///
/// * **info** is an object of type [`MessageInfo`].
///
/// * **assets** is an array with two objects of type [`Asset`]. These are the assets available in the pool.
///
/// * **slippage_tolerance** is an [`Option`] field of type [`Decimal`]. It is used to specify how much
/// the pool price can move until the provide liquidity transaction goes through.
///
/// * **receiver** is an [`Option`] field of type [`String`]. This is the receiver of the LP tokens.
/// If no custom receiver is specified, the pair will mint LP tokens for the function caller.
// NOTE - the address that wants to provide liquidity should approve the pair contract to pull its relevant tokens.
pub fn provide_liquidity(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    offer_assets: Vec<Asset>,
    slippage_tolerance: Option<Decimal>,
    receiver: Option<String>,
) -> Result<Response, ContractError> {
    
    for asset in &offer_assets {
        // Check assets for valid formatting
        asset.info.check(deps.api)?;

        // Validate amount of native tokens transferred
        asset.assert_sent_native_token_balance(&info)?;
    }


    // Load basket and gather assets
    let mut basket: Basket = BASKET.load(deps.storage)?;
    let mut basket_assets = basket.assets.clone();

    let mut messages: Vec<CosmosMsg> = vec![];
    for (i, asset) in basket_assets.iter_mut().enumerate() {
        // If the asset is a token contract, then we need to execute a TransferFrom msg to receive basket_assets
        if let AssetInfo::Token { contract_addr, .. } = &asset.info {
            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: contract_addr.to_string(),
                msg: to_binary(&Cw20ExecuteMsg::TransferFrom {
                    owner: info.sender.to_string(),
                    recipient: env.contract.address.to_string(),
                    amount: offer_assets[i].amount,
                })?,
                funds: vec![],
            }));
        }
    }

    // Grab relevant asset assets in basket, zipped with price
    let offer_assets_with_price: Vec<(BasketAsset, Price)> = {
        let mut v: Vec<(BasketAsset, Price)>= vec![];

        for asset in &offer_assets {
            v.push(match basket.assets
                .iter()
                .zip(basket.get_prices(&deps.querier)?)
                .find(|(asset, _price)| asset.info.equal(&asset.info)) {
                    Some((asset, price)) => (asset.clone(), price.clone()),
                    None => return Err(ContractError::AssetNotInBasket)
                }
            )
        }
        v
    };

    // Price of one token --> Value of assets
    let offer_asset_values: Vec<Uint128> = offer_assets_with_price
        .iter()
        .map(|(basket_asset, price)| 
            safe_price_to_Uint128(
                    Price::price_basket(
                    &[(
                        *price, 
                        safe_u128_to_i64(basket_asset.available_reserves.u128()).unwrap() +
                            safe_u128_to_i64(basket_asset.occupied_reserves.u128()).unwrap(), 
                        -(query_token_precision(&deps.querier, &basket_asset.info).unwrap() as i32)
                    )],
                    USD_VALUE_PRECISION
                ).unwrap()
            )
        )
        .collect();
    let initial_aum_value: Uint128 = safe_price_to_Uint128(basket.calculate_aum(&deps.querier)?);

    // Value of user deposits
    let user_deposit_values: Vec<Uint128> = offer_assets_with_price
        .iter()
        .enumerate()
        .map(|(i, (offer_asset_with_price, price))| 
            safe_price_to_Uint128(
                    {
                        assert!(offer_assets[i].info.equal(&offer_asset_with_price.info));
                        Price::price_basket(
                            &[(
                                *price, 
                                safe_u128_to_i64(
                                    offer_asset_with_price.available_reserves.u128() + offer_asset_with_price.occupied_reserves.u128()
                                ).unwrap(),
                                -(query_token_precision(&deps.querier, &offer_asset_with_price.info).unwrap() as i32)
                            )],
                            USD_VALUE_PRECISION
                    ).unwrap()
                }
            )
        )
        .collect();
    let total_user_deposit_value: Uint128 = user_deposit_values.iter().sum();
    
    // Begin calculating amount of LP token to mint
    let new_aum_value = initial_aum_value + total_user_deposit_value;

    // Get price feeds, prices of basket assets
    let price_feeds: Vec<PriceFeed> = basket.get_price_feeds(&deps.querier)?;
    let prices: Vec<Price> = basket.get_prices(&deps.querier)?;

    // Retrieve LP token supply
    let lp_supply: Uint128 = query_supply(&deps.querier, basket.lp_token_address.clone())?;

    println!("total_user_deposit_value: {}", total_user_deposit_value);
    // Calculate share -  What exactly is share?
    let tokens_to_mint: Uint128 = if lp_supply.is_zero() {

        // Handle deposit into empty basket at 1:1 USD_VALUE_PRECISION mint. First deposit gets zero fees
        total_user_deposit_value

    } else {

        // Handle deposit into nonempty basket

        // TODO: do we need to check for slippage for any reason if we use oracles? Maybe if user doesn't want to pay max bps fee?
        // Assert slippage tolerance
        // assert_slippage_tolerance(slippage_tolerance, &deposits, &assets)?;

        // exchange rate is (lp supply) / (aum)
        // here we value * rate = value * lp supply / aum, safely
        // then, we reduce fees by doing gross * ( 10000 - deposit_fee ) / 10000
        let pre_fee: Uint128 = total_user_deposit_value.multiply_ratio(lp_supply, initial_aum_value);

        // Gather bps for all fees
        let fee_bps: Vec<Uint128> = calculate_fee_basis_points(
            initial_aum_value, 
            &basket, 
            &offer_asset_values, 
            &user_deposit_values,
            &basket.match_basket_assets(&offer_assets.to_asset_info()),
            Action::Offer
        );
        let fees: Vec<Uint128> = user_deposit_values
            .iter()
            .zip(fee_bps)
            .map(|(value, bps)| value.multiply_ratio(BASIS_POINTS_PRECISION - bps, BASIS_POINTS_PRECISION))
            .collect();

        let post_fee = pre_fee - fees.iter().sum::<Uint128>();
        post_fee
    };

    // TODO: I think this is where we subtract fees from share. I may be wrong.
    // Also I think first depositor is charged no fee if we do it here because they just get minted less but they own 100% of lp token.
    // Maybe we take difference and mint it to some fee wallet?

    offer_assets.iter().for_each(|offer_asset| match basket.assets.iter_mut()
    .find(|asset| offer_asset.info.equal(&asset.info)) {
            Some(offer_basket_asset) => { 
                offer_basket_asset.available_reserves += offer_asset.amount 
            },
            None => {},
        });

    BASKET.save(deps.storage, &basket)?;


    // Mint LP tokens for the sender or for the receiver (if set)
    let receiver = receiver.unwrap_or_else(|| info.sender.to_string());
    messages.extend(mint_liquidity_token_message(
        deps.as_ref(),
        &basket,
        env.clone(),
        validate_addr(deps.api, &receiver)?,
        tokens_to_mint,
    ).map_err(|_| ContractError::LpMintFailed)?);

    // Return response with attributes
    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("action", "provide_liquidity"),
        attr("sender", info.sender.as_str()),
        attr("receiver", receiver.as_str()),
        attr("offer_asset", format!("{:?}", &offer_assets)),
        attr("tokens_to_mint", tokens_to_mint.to_string()),
    ]))
}

/// ## Description
/// Mint LP tokens for a beneficiary and auto stake the tokens in the Generator contract (if auto staking is specified).
/// # Params
/// * **deps** is an object of type [`Deps`].
///
/// * **config** is an object of type [`Config`].
///
/// * **env** is an object of type [`Env`].
///
/// * **recipient** is an object of type [`Addr`]. This is the LP token recipient.
///
/// * **amount** is an object of type [`Uint128`]. This is the amount of LP tokens that will be minted for the recipient.
///
/// * **auto_stake** is the field of type [`bool`]. Determines whether the newly minted LP tokens will
/// be automatically staked in the Generator on behalf of the recipient.
fn mint_liquidity_token_message(
    deps: Deps,
    basket: &Basket,
    env: Env,
    recipient: Addr,
    amount: Uint128,
) -> Result<Vec<CosmosMsg>, ContractError> {

    // Retrieve lp token contract address
    let lp_token = basket.lp_token_address.clone();

    // Mint to Recipient
    return Ok(vec![CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: lp_token.to_string(),
        msg: to_binary(&Cw20ExecuteMsg::Mint {
            recipient: recipient.to_string(),
            amount,
        })?,
        funds: vec![],
    })]);
}



pub fn safe_u128_to_i64(input: u128) -> Result<i64, ContractError> {
    let output = input as i64;
    if output as u128 == input {
        return Ok(output)
    } else {
        return Err(ContractError::FailedCast)
    }
}

pub fn safe_i64_to_u128(input: i64) -> Result<u128, ContractError> {
    let output = input as u128;
    if output as i64 == input {
        return Ok(output)
    } else {
        return Err(ContractError::FailedCast)
    }
}



pub fn safe_price_to_Uint128(
    price: Price,
) -> Uint128 {
    return Uint128::new(price.price as u128);

    // Positive price
    assert!(price.price >= 0, "amount must be non-negative");
    let amount: u128 = price.price as u128;

    if price.expo >= 0 {

        // Deal with non-negative exponent
        let expo = price.expo as u32;
        Uint128::from(amount).multiply_ratio(10_u32.pow(expo), 1_u32)
    } else {

        // Deal with negative exponent
        let expo = price.expo.abs() as u32;
        Uint128::from(amount).multiply_ratio(1_u32, 10_u32.pow(expo))
    }

}
