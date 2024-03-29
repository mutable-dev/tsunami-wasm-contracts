use crate::{
    asset::{addr_validate_to_lower, Asset, AssetInfo, PricedAsset},
    error::ContractError,
    msg::*,
    querier::query_supply,
    state::{Basket, BasketAsset, ToAssetInfo, BASKET},
};
#[allow(unused_imports)]
use cosmwasm_std::{
    attr, entry_point, from_binary, to_binary, Addr, Binary, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Reply, ReplyOn, Response, StdError, StdResult, SubMsg, Uint128, Uint256,
    WasmMsg,
};
use cw2::set_contract_version;
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg, MinterResponse};
use protobuf::Message;
use pyth_sdk_terra::Price;

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
        ExecuteMsg::DepositLiquidity {
            assets,
            slippage_tolerance,
            receiver,
        } => provide_liquidity(deps, env, info, assets, slippage_tolerance, receiver),
        ExecuteMsg::Receive( msg ) => receive_cw20(deps, env, info, msg),
        ExecuteMsg::Swap {
            sender,
            offer_asset,
            belief_price,
            max_spread,
            to,
            ask_asset,
        } => swap(
            deps,
            env,
            info,
            sender,
            offer_asset,
            belief_price,
            max_spread,
            to,
            ask_asset,
        ),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    let mut basket: Basket = BASKET.load(deps.storage)?;

    if basket.lp_token_address != Addr::unchecked("") {
        return Err(ContractError::Unauthorized);
    }

    let data = msg.result.unwrap().data.expect("Could not retrieve Reply msg.data when replying");
    let res: MsgInstantiateContractResponse =
        Message::parse_from_bytes(data.as_slice()).map_err(|_| {
            StdError::parse_err("MsgInstantiateContractResponse", "failed to parse data in reply")
        })?;

    basket.lp_token_address = addr_validate_to_lower(deps.api, res.get_contract_address())?;

    BASKET.save(deps.storage, &basket)?;

    Ok(Response::new().add_attribute("liquidity_token_addr", basket.lp_token_address))
}

pub fn withdraw_liquidity(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    sender: Addr,
    amount: Uint128,
    ask_asset: AssetInfo,
) -> Result<Response, ContractError> {
    // Load Basket
    let basket: Basket = BASKET.load(deps.storage)?;

    // Abort if not from basket lp token contract
    if info.sender != basket.lp_token_address {
        return Err(ContractError::Unauthorized);
    }

    // Retrieve ask asset
    let ask_asset = basket.assets
        .iter()
        .find(|asset| asset.info == ask_asset)
        .ok_or(ContractError::AssetNotInBasket)?;

    let mut ask_asset = PricedAsset::new(Asset{info: ask_asset.info.clone(), amount: Uint128::zero()}, ask_asset.clone());

    // Calculate gross asset return value
    let mut redemption_value: Uint128 =
        basket.withdraw_amount(amount, &deps.querier)?;

    // Calculate fee_bps
    let initial_aum_value: Uint128 = basket.calculate_aum(&deps.querier)?.to_Uint128(USD_VALUE_PRECISION)?;
    let fee_bps: Uint128 = calculate_fee_basis_points(
        initial_aum_value,
        &basket,
        &[ask_asset.query_contract_value(&deps.querier)?],
        &vec![redemption_value],
        &vec![ask_asset.basket_asset.clone()],
        Action::Ask,
    )[0];

    // Update refund_asset with fee
    redemption_value =
        redemption_value.multiply_ratio(BASIS_POINTS_PRECISION - fee_bps, BASIS_POINTS_PRECISION);

    let decimals = ask_asset.query_decimals(&deps.querier)?;
    let redemption_amount = redemption_value.multiply_ratio(Uint128::from(10_u64).pow(decimals as u32), ask_asset.query_price(&deps.querier)?.to_Uint128(-decimals)?);
    let redemption_asset = Asset {
        amount: redemption_amount,
        info: ask_asset.asset.info,
    };

    // Update the asset info
    let messages: Vec<CosmosMsg> = vec![
        redemption_asset
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
        attr("redemption_asset", format!("{}", redemption_asset)),
        attr("fee_bps", &fee_bps.to_string()),
    ];

    Ok(Response::new()
        .add_messages(messages)
        .add_attributes(attributes))
}

/// Produces unit price of USD, in units of `USD_VALUE_PRECISION`
pub fn get_unit_price() -> Price {
    Price {
        price: 1, //10_i64.pow(-USD_VALUE_PRECISION as u32),
        expo: USD_VALUE_PRECISION,
        conf: 0,
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
                symbol: "TLP".to_string(),
                decimals: LP_DECIMALS,
                initial_balances: vec![],
                mint: Some(MinterResponse {
                    minter: env.contract.address.to_string(),
                    cap: None,
                }),
            })
            .expect("failed to convert InstantiateLpMsg to binary."),
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

fn build_assets(msg: &InstantiateMsg) -> Vec<BasketAsset> {
    let mut assets = Vec::new();
    for asset in msg.assets.clone() {
        assets.push(BasketAsset::new(asset));
    }
    assets
}

fn check_assets(assets: &Vec<InstantiateAssetInfo>) -> Result<u64, ContractError> {
    let mut asset_names: Vec<String> = Vec::new();
    for asset in assets {
        if asset_names.contains(&asset.address.to_string()) {
            return Err(ContractError::DuplicateAssetAssertion);
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
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
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
                Some(addr_validate_to_lower(deps.api, &to_addr)?)
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
        Ok(Cw20HookMsg::WithdrawLiquidity { asset }) => withdraw_liquidity(
            deps,
            env,
            info,
            Addr::unchecked(cw20_msg.sender),
            cw20_msg.amount,
            asset,
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
    _belief_price: Option<Decimal>,
    _max_spread: Option<Decimal>,
    to: Option<Addr>,
    ask_asset: AssetInfo,
) -> Result<Response, ContractError> {
    // Ensure native token was sent
    offer_asset.assert_sent_native_token_balance(&info)?;

    // Load basket singleton, get assets
    let mut basket: Basket = BASKET.load(deps.storage)?;

    let mut messages: Vec<CosmosMsg> = vec![];
    for asset in basket.assets.iter_mut() {
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

    let offer_basket_asset = match basket.assets.iter().find(|asset| {asset.info == offer_asset.info}) {
        Some(asset) => asset.clone(),
        None => return Err(ContractError::AssetNotInBasket),
    };

    let ask_basket_asset = match basket.assets.iter().find(|asset| {asset.info == ask_asset}) {
        Some(asset) => asset.clone(),
        None => return Err(ContractError::AssetNotInBasket),
    };

    let mut offer_asset = PricedAsset::new(offer_asset, offer_basket_asset);
    let mut ask_asset = PricedAsset::new(Asset{info: ask_asset, amount: Uint128::zero()}, ask_basket_asset);

    let initial_aum_value = Uint128::new(basket.calculate_aum(&deps.querier)?.pyth_price.price as u128);
    let user_offer_value = offer_asset.query_value(&deps.querier)?;
    let offer_fee_bps: Uint128 = calculate_fee_basis_points(
        initial_aum_value,
        &basket,
        &[offer_asset.query_contract_value(&deps.querier)?],
        &vec![user_offer_value],
        &[offer_asset.basket_asset.clone()],
        Action::Offer,
    )[0];
    let ask_fee_bps: Uint128 = calculate_fee_basis_points(
        initial_aum_value,
        &basket,
        &[ask_asset.query_contract_value(&deps.querier)?],
        &vec![user_offer_value],
        &[ask_asset.basket_asset.clone()],
        Action::Ask,
    )[0];

    // Calculate post-fee USD value, then convert USD value to number of tokens.
    let return_asset_value = user_offer_value.multiply_ratio(
        BASIS_POINTS_PRECISION - ask_fee_bps - offer_fee_bps,
        BASIS_POINTS_PRECISION,
    );
    // Get value of ask per unit usd, e.g. microUSD
    let ask_per_unit_usd = ask_asset.query_price(&deps.querier)?.pyth_price.price as u128;
    // The price of a lamport is 10^ask_decimals lower, so multiply refund_value by appropriate power of 10 then divide by ask price
    let return_asset_amount =
        return_asset_value.multiply_ratio(10_u128.pow(ask_asset.query_decimals(&deps.querier)? as u32), ask_per_unit_usd);

    // Construct asset type and convert to message to `to` or `sender`
    let return_asset = Asset {
        info: ask_asset.asset.info.clone(),
        amount: return_asset_amount,
    };
    let receiver = to.unwrap_or_else(|| sender.clone());
    let messages: Vec<CosmosMsg> = vec![return_asset.into_msg(&deps.querier, receiver.clone())?];

    match basket
        .assets
        .iter_mut()
        .find(|asset| offer_asset.asset.info.equal(&asset.info))
    {
        Some(offer_basket_asset) => offer_basket_asset.available_reserves += offer_asset.asset.amount,
        None => {}
    }

    match basket
        .assets
        .iter_mut()
        .find(|asset| ask_asset.asset.info.equal(&asset.info))
    {
        Some(offer_asset) => offer_asset.available_reserves -= return_asset_amount,
        None => {}
    }

    // Save state
    BASKET.save(deps.storage, &basket)?;

    Ok(Response::new()
        .add_messages(
            // 1. send collateral tokens from the contract to a user
            // 2. send inactive commission fees to the Maker contract
            messages,
        )
        .add_attribute("action", "swap")
        .add_attribute("sender", sender.as_str())
        .add_attribute("receiver", receiver.as_str())
        .add_attribute("offer_asset", offer_asset.asset.info.to_string())
        .add_attribute("ask_asset", ask_asset.asset.info.to_string())
        .add_attribute("offer_amount", offer_asset.asset.amount.to_string())
        .add_attribute("return_asset_amount", return_asset_amount.to_string())
        .add_attribute("offer_bps", offer_fee_bps.to_string())
        .add_attribute("ask_bps", ask_fee_bps.to_string()))
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
///
/// # Arguments
///
/// * `initial_aum_value` - The total value (normalized in USD) of the Basket's assets
/// * `basket` - The Basket of assets being traded against
/// * `initial_reserve_values` - The reserve values (normalized in USD) for each BasketAsset
/// being traded against. This includes occupied and unoccupied assets in the pool.
/// * `offer_or_ask_values` - The USD amount the user wants to trade for each BasketAsset
/// * `offer_or_ask_assets` - The BasketAsset's that are being traded against
/// * `action` - Offer|Ask used to determine if the user is buying or selling the assets,
/// respectively.
///
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
    initial_reserve_values: &[Uint128],
    offer_or_ask_values: &Vec<Uint128>,
    offer_or_ask_assets: &[BasketAsset],
    action: Action,
) -> Vec<Uint128> {
    // Compute new aum_value
    let new_aum_value: Uint128 = match action {
        Action::Offer => initial_aum_value + offer_or_ask_values.iter().sum::<Uint128>(),
        Action::Ask => initial_aum_value - offer_or_ask_values.iter().sum::<Uint128>(),
    };

    // Compute updated reserve value by adding or subtracting diff_usd_value based on action
    let next_reserve_usd_values: Vec<Uint128> = match action {
        Action::Offer => initial_reserve_values
            .iter()
            .zip(offer_or_ask_values)
            .map(|(&a, &b)| a + b)
            .collect(),
        Action::Ask => initial_reserve_values
            .iter()
            .zip(offer_or_ask_values)
            .map(|(&a, &b)| a.checked_sub(b).expect("ask too large"))
            .collect(),
    };

    let mut fee_bps: Vec<Uint128> = vec![];
    for i in 0..offer_or_ask_assets.len() {
        let offer_or_ask_asset = offer_or_ask_assets[i].clone();
        let initial_reserve_value = initial_reserve_values[i];
        let next_reserve_usd_value = next_reserve_usd_values[i];

        // First depositor should not be hit with a fee
        if  initial_reserve_value.is_zero() {
            fee_bps.push(Uint128::zero());
            break
        }

        // Compute target value based on weight, so that we may compare to the updated value
        let initial_target_lp_usd_value: Uint128 = initial_aum_value
            .multiply_ratio(offer_or_ask_asset.token_weight, basket.get_total_weights());
        let new_target_lp_usd_value: Uint128 = new_aum_value
            .multiply_ratio(offer_or_ask_asset.token_weight, basket.get_total_weights());

        // Calculate the initial and new distance from the target value
        let initial_distance: Uint128 = initial_target_lp_usd_value.max(initial_reserve_value)
            - initial_target_lp_usd_value.min(initial_reserve_value);
        let new_distance: Uint128 = new_target_lp_usd_value.max(next_reserve_usd_value)
            - new_target_lp_usd_value.min(next_reserve_usd_value);
        
        let improvement = 
            Uint256::from_uint128(new_distance) * Uint256::from_uint128(initial_target_lp_usd_value) <=
            Uint256::from_uint128(initial_distance) * Uint256::from_uint128(new_target_lp_usd_value);

        if improvement {
            fee_bps.push(BASE_FEE_IN_BASIS_POINTS.multiply_ratio(
                initial_target_lp_usd_value - initial_distance.min(new_target_lp_usd_value),
                initial_target_lp_usd_value,
            ));
        } else {
            fee_bps.push(BASE_FEE_IN_BASIS_POINTS + PENALTY_IN_BASIS_POINTS.multiply_ratio(
                new_distance.min(new_target_lp_usd_value),
                new_target_lp_usd_value,
            ));
        }
    }
    fee_bps
}

pub enum Action {
    Offer,
    Ask,
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
    _slippage_tolerance: Option<Decimal>,
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
    let mut offer_priced_assets: Vec<PricedAsset> = {
        let mut v: Vec<PricedAsset> = vec![];

        for offer_asset in &offer_assets {
            v.push(
                match basket.assets
                    .iter()
                    .find(|asset| asset.info.equal(&offer_asset.info))
                {
                    Some(asset) => PricedAsset::new(
                        offer_asset.clone(),
                        asset.clone(),
                    ),
                    None => return Err(ContractError::AssetNotInBasket),
                },
            )
        }
        v
    };

    // Price of one token --> Value of assets
    let offer_asset_values_in_contract = match offer_priced_assets
        .iter_mut()
        .map(|asset| {
            asset.query_contract_value(&deps.querier)
        })
        .collect::<Result<Vec<_>, ContractError>>() {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
    let initial_aum_value: Uint128 = basket.calculate_aum(&deps.querier)?.to_Uint128(USD_VALUE_PRECISION)?;

    // Value of user deposits
    let user_deposit_values: Vec<Uint128> = match offer_priced_assets
        .iter_mut()
        .map(|asset| {
            asset.query_value(&deps.querier)
        })
        .collect::<Result<Vec<_>, ContractError>>() {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
    let total_user_deposit_value: Uint128 = user_deposit_values.iter().sum();

    // Retrieve LP token supply
    let lp_supply: Uint128 = query_supply(&deps.querier, basket.lp_token_address.clone())?;

    // Calculate share -  What exactly is share?
    let tokens_to_mint: Uint128 = if lp_supply.is_zero() {
        // Handle deposit into empty basket at 1:1 USD_VALUE_PRECISION mint. First deposit gets zero fees
        total_user_deposit_value.multiply_ratio(
            10_u128.pow(LP_DECIMALS as u32),
            10_u128.pow(-USD_VALUE_PRECISION as u32),
        )
    } else {
        // Handle deposit into nonempty basket

        // This is the number of tokens to mint before any fees
        let pre_fee: Uint128 =
            total_user_deposit_value.multiply_ratio(lp_supply, initial_aum_value);

        // Gather fee bps for all deposit assets
        let fee_bps: Vec<Uint128> = calculate_fee_basis_points(
            initial_aum_value,
            &basket,
            &offer_asset_values_in_contract,
            &user_deposit_values,
            &basket.match_basket_assets(&offer_assets.to_asset_info()),
            Action::Offer,
        );

        // Calculate all fees: fee per deposit asset
        let fees: Vec<Uint128> = user_deposit_values
            .iter()
            .zip(fee_bps)
            .map(|(value, bps)| {
                value.multiply_ratio(BASIS_POINTS_PRECISION - bps, BASIS_POINTS_PRECISION)
            })
            .collect();

        let post_fee = pre_fee - fees.iter().sum::<Uint128>();
        post_fee.multiply_ratio(
            10_u128.pow(LP_DECIMALS as u32),
            10_u128.pow(-USD_VALUE_PRECISION as u32),
        )
    };


    // Update 
    offer_assets.iter().for_each(|offer_asset| {
        match basket
            .assets
            .iter_mut()
            .find(|asset| offer_asset.info.equal(&asset.info))
        {
            Some(offer_basket_asset) => offer_basket_asset.available_reserves += offer_asset.amount,
            None => { panic!("{}", ContractError::AssetNotInBasket) }
        }
    });

    // Mint LP tokens for the sender or for the receiver (if set)
    let receiver = receiver.unwrap_or_else(|| info.sender.to_string());
    messages.extend(
        mint_liquidity_token_message(
            deps.as_ref(),
            &basket,
            env,
            addr_validate_to_lower(deps.api, &receiver)?,
            tokens_to_mint,
        )
        .map_err(|_| ContractError::LpMintFailed)?,
    );

    BASKET.save(deps.storage, &basket)?;

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
    _deps: Deps,
    basket: &Basket,
    _env: Env,
    recipient: Addr,
    amount: Uint128,
) -> Result<Vec<CosmosMsg>, ContractError> {
    // Retrieve lp token contract address
    let lp_token = basket.lp_token_address.clone();

    // Mint to Recipient
    Ok(vec![CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: lp_token.to_string(),
        msg: to_binary(&Cw20ExecuteMsg::Mint {
            recipient: recipient.to_string(),
            amount,
        })?,
        funds: vec![],
    })])
}

// TODO: should pass in an enum that is either offer, ask, USD, and check the expo of the price going in
#[allow(non_snake_case)]
pub fn safe_price_to_Uint128(price: Price, expected_expo: i32) -> Result<Uint128, ContractError> {

    // Check for positive price
    if price.price < 0 { return Err(ContractError::NegativePrice) }

    // Check for expected expo
    if price.expo != expected_expo { return Err(ContractError::IncorrectDecimals { expo: price.expo, expected_expo }) }
    
    Ok(Uint128::new(price.price as u128))
}
