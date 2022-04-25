use crate::{
    error::ContractError,
    msg::*,
    asset::{AssetInfo, Asset, addr_validate_to_lower},
    state::{Basket, BasketAsset, BASKET},
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


/// Contract name that is used for migration.
const CONTRACT_NAME: &str = "tsunami-basket";
/// Contract version that is used for migration.
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const INSTANTIATE_BASKET_REPLY_ID: u64 = 1;
const BASIS_POINTS_PRECISION: Uint128 = Uint128::new(10_000);
const BASE_FEE_IN_BASIS_POINTS: Uint128 = Uint128::new(15);
const PENALTY_IN_BASIS_POINTS: Uint128 = Uint128::new(15);


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

    // Retrieve ask pool
    let pools = basket.get_pools();
    let ask_pool: (Asset, Price) = match pools.iter().zip(basket.get_prices(&deps.querier)?)
        .find(|(pool, _price)| ask_asset.info.equal(&pool.info)) {
            Some((pool, price)) => (pool.clone(), price.clone()),
            None => return Err(ContractError::AssetNotInBasket)
    };


    // Calculate gross asset return
    let price_feeds: Vec<PriceFeed> = basket.get_price_feeds(&deps.querier)?;
    let prices: Vec<Price> = basket.get_prices(&deps.querier)?;
    let ask_asset_aum = basket.calculate_aum(&price_feeds, &ask_asset.info);
    let mut refund_asset = basket.withdraw_amount(amount, &price_feeds, ask_asset.info.clone())?;

    // Calculate fee_bps
    let fee_bps = calculate_fee_basis_points(
        basket.calculate_aum(&price_feeds, &refund_asset.info)?.aum,
        basket.assets.iter().find(|asset| asset.info == ask_asset.info).unwrap(),
        basket.total_weights, 
        // TODO: This is "price". What goes here? putting ask_asset price for now
        Uint128::from(safe_i64_to_u128(ask_pool.1.price)?),
        ask_pool.1.expo.try_into().unwrap(),
        refund_asset.amount, 
        false
    );

    // Update refund_asset
    refund_asset.amount = refund_asset.amount.multiply_ratio(BASIS_POINTS_PRECISION - fee_bps, BASIS_POINTS_PRECISION);

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
) -> Result<Addr, ContractError> {
    Ok(Addr::unchecked(sender.clone()))
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

    // Load basket singleton
    let mut basket: Basket = BASKET.load(deps.storage)?;

    // If the asset balance is already increased, we should subtract the user deposit from the pool amount
    let pools: Vec<Asset> = basket
        .get_pools()
        .iter()
        .map(|p| {
            let mut p = p.clone();
            if p.info.equal(&offer_asset.info) {
                p.amount = p.amount.checked_sub(offer_asset.amount).unwrap();
            }

            p
        })
        .collect();

    // Grab relevant asset pools in basket, zipped with price
    let offer_pool: (Asset, Price) = match pools.iter().zip(basket.get_prices(&deps.querier)?)
        .find(|(pool, _price)| offer_asset.info.equal(&pool.info)) {
            Some((pool, price)) => (pool.clone(), price.clone()),
            None => return Err(ContractError::AssetNotInBasket)
    };
    let ask_pool: (Asset, Price) = match pools.iter().zip(basket.get_prices(&deps.querier)?)
        .find(|(pool, _price)| ask_asset.equal(&pool.info)) {
            Some((pool, price)) => (pool.clone(), price.clone()),
            None => return Err(ContractError::AssetNotInBasket)
    };

    // Get price feeds, prices of basket assets
    let price_feeds: Vec<PriceFeed> = basket.get_price_feeds(&deps.querier)?;
    let prices: Vec<Price> = basket.get_prices(&deps.querier)?;

    // Get price of ask in offer to at least SIG_FIGS sig figs (fails otherwise)
    const SIG_FIGS: i32 = 6;
    let ask_usd: Price = ask_pool.1;
    let offer_usd: Price = offer_pool.1;
    let ask_offer: Price = ask_usd.get_price_in_quote(&offer_usd, SIG_FIGS).unwrap();

    // Compute gross = offer * (offer aum / ask aum) to at least SIG_FIGS sig figs (fails otherwise)
    // TODO: check/verify which decimals this should be. Probably should be token decimals so that offer_asset.amount is just "lamports".
    let todo_decimals: i32 = query_token_precision(&deps.querier, &offer_asset.info)?.try_into().unwrap();
    let gross_output_asset_out: Price = Price::price_basket(&[(ask_offer, safe_u128_to_i64(offer_asset.amount.u128())?, todo_decimals)], todo_decimals).unwrap();

    // Compute offer fee 
    // TODO: redo exponent stuff here
    let offer_aum = basket.calculate_aum(&price_feeds, &offer_asset.info)?;
    let offer_fee_in_basis_points: Uint128 = calculate_fee_basis_points(
        offer_aum.aum,
        basket.assets.iter().find(|asset| asset.info == offer_asset.info).unwrap(),
        basket.total_weights, 
        Uint128::new(offer_aum.price as u128),
        offer_aum.exponent as u32,
        offer_asset.amount, 
        true
    );

    // Compute ask fee
    // TODO: redo exponent stuff here
    let ask_aum = basket.calculate_aum(&price_feeds, &ask_asset)?;
    let ask_fee_in_basis_points = calculate_fee_basis_points(
        ask_aum.aum, 
        basket.assets.iter().find(|asset| asset.info == ask_asset).unwrap(),
        basket.total_weights, 
        Uint128::new(ask_aum.price as u128),
        ask_aum.exponent as u32,
        safe_i64_expo_to_u128(gross_output_asset_out.price, gross_output_asset_out.expo),
        false
    );

    // TODO: check if i32 -> u128 cast is safe (fails appropriately)
    // TODO: Ensure this number of decimals is appropriate
    let net_output_asset_out = Uint128::from(safe_i64_expo_to_u128(gross_output_asset_out.price, gross_output_asset_out.expo))
        .multiply_ratio(
            BASIS_POINTS_PRECISION - ask_fee_in_basis_points - offer_fee_in_basis_points,
            BASIS_POINTS_PRECISION
    );


    // Compute the tax for the receiving asset (if it is a native one)
    let return_asset = Asset {
        info: ask_pool.0.info.clone(),
        amount: net_output_asset_out.try_into().expect("Failed Uint256 -> Uint128"),
    };
    let receiver = to.unwrap_or_else(|| sender.clone());
    let mut messages: Vec<CosmosMsg> =
        vec![return_asset.into_msg(&deps.querier, receiver.clone())?];


    // 
    Ok(Response::new()
        .add_messages(
            // 1. send collateral tokens from the contract to a user
            // 2. send inactive commission fees to the Maker ontract
            messages,
        )
        .add_attribute("action", "swap")
        .add_attribute("sender", sender.as_str())
        .add_attribute("receiver", receiver.as_str())
        .add_attribute("offer_asset", offer_asset.info.to_string())
        .add_attribute("ask_asset", ask_pool.0.info.to_string())
        .add_attribute("offer_amount", offer_asset.amount.to_string())
        .add_attribute("return_amount", net_output_asset_out.to_string())
        //.add_attribute("spread_amount", spread_amount.to_string())
        //.add_attribute("commission_amount", commission_amount.to_string())
        //.add_attribute("maker_fee_amount", maker_fee_amount.to_string()))
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
	aum: Uint128,
	basket_asset: &BasketAsset, 
	total_weight: Uint128, 
	price: Uint128,
	exponent: u32,
	offer_or_ask_amount: Uint128,
	increment: bool
) -> Uint128 {
	let current_reserves = basket_asset.pool_reserves;
	let initial_reserve_usd_value = (current_reserves)
        .checked_mul(price)
		.unwrap()
        // TODO test for this exponent
		.checked_div(Uint128::new(10).pow(exponent))
		.unwrap();

	let diff_usd_value = offer_or_ask_amount
		.checked_mul(price)
		.unwrap()
        // TODO test for this exponent
		.checked_div(Uint128::new(10).pow(exponent))
		.unwrap();

	let next_reserve_usd_value = if increment { 
		initial_reserve_usd_value + diff_usd_value 
	} else { 
		max(initial_reserve_usd_value - diff_usd_value, Uint128::new(0))
	};
	
	let target_lp_usd_value = basket_asset.token_weight
		.checked_mul(aum)
		.unwrap()
		.checked_div(total_weight)
		.unwrap();

	if target_lp_usd_value == Uint128::new(0) {
		return BASE_FEE_IN_BASIS_POINTS;
	}

	let initial_usd_from_target = if initial_reserve_usd_value > target_lp_usd_value { 
		initial_reserve_usd_value - target_lp_usd_value
	} else { target_lp_usd_value - initial_reserve_usd_value  };

	let next_usd_from_target = if next_reserve_usd_value > target_lp_usd_value { 
		next_reserve_usd_value - target_lp_usd_value
	} else { target_lp_usd_value - next_reserve_usd_value  };

	// action improves target balance
	if next_usd_from_target < initial_usd_from_target {
		let rebate_bps = BASE_FEE_IN_BASIS_POINTS
			.checked_mul(initial_usd_from_target)
			.unwrap()
			.checked_div(target_lp_usd_value )
			.unwrap();
		return if rebate_bps >= BASE_FEE_IN_BASIS_POINTS  {
			Uint128::zero() 
		} else { 
			BASE_FEE_IN_BASIS_POINTS - rebate_bps
		};
	} else if next_usd_from_target == initial_usd_from_target {
		return BASE_FEE_IN_BASIS_POINTS
	}

	let mut average_diff = (initial_usd_from_target + next_usd_from_target)/Uint128::new(2);
	if average_diff > target_lp_usd_value {
		average_diff = target_lp_usd_value ;
	}

    // TODO: perhaps do safer U256 mul + div -> U128?
	let penalty = PENALTY_IN_BASIS_POINTS * average_diff / target_lp_usd_value ;
	return BASE_FEE_IN_BASIS_POINTS + penalty
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
    assets: Vec<Asset>,
    slippage_tolerance: Option<Decimal>,
    receiver: Option<String>,
) -> Result<Response, ContractError> {

    // Check assets for valid formatting
    for asset in &assets{
        asset.info.check(deps.api)?;
    }

    // Validate amount of native tokens transferred
    for asset in assets.iter() {
        asset.assert_sent_native_token_balance(&info)?;
    }

    let mut basket: Basket = BASKET.load(deps.storage)?;
    
    // Retrieve each asset pool, and order the deposit assets in the same order as the pools
    let deposits: Vec<Asset> = {
        let mut v = vec![];
        for basket_asset in &basket.assets {
            v.push(assets
                .iter()
                .find(|a| a.info.equal(&basket_asset.info))
                .map(|a| a.clone())
                .unwrap_or(Asset { 
                    info: basket_asset.info.clone(), 
                    amount: Uint128::new(0) 
                }));
        }
        v
    };

    let mut messages: Vec<CosmosMsg> = vec![];

    // Generate the messages to transfer deposited nonnative assets to the contract address
    for (i, basket_asset) in basket.assets.iter_mut().enumerate() {
        match &basket_asset.info {
            AssetInfo::Token { contract_addr } => {
                messages.push(CosmosMsg::Wasm(WasmMsg::Execute { 
                    contract_addr: contract_addr.to_string(), 
                    msg: to_binary(&Cw20ExecuteMsg::TransferFrom {
                        owner: info.sender.to_string(),
                        recipient: env.contract.address.to_string(),
                        amount: deposits[i].amount,
                    })?,
                    funds: vec![],
                }));
            },
            _ => {},
        }
    }


    // Begin calculating amount of LP token to mint

    // Get price feeds, prices of basket assets
    let price_feeds: Vec<PriceFeed> = basket.get_price_feeds(&deps.querier)?;
    let prices: Vec<Price> = basket.get_prices(&deps.querier)?;

    // Calculate value of user deposits
    let mut individual_amounts: Vec<(Price, i64, i32)> = vec![];
    let amounts: &[(Price, i64, i32)] = {
        for i in 0..deposits.len() {
            individual_amounts.push((prices[i], safe_u128_to_i64(deposits[i].amount.u128())?, prices[i].expo));
        }
        individual_amounts.as_slice()
    };
    let result_expo = -9; // TODO: ensure this is what we want. I think this means we price the basket down to 1e-9 USD
    let user_deposit_values: Vec<Uint128> = {
        let mut v = vec![];
        for amount in &individual_amounts {
            v.push(
                Uint128::new(safe_i64_to_u128(Price::price_basket(&[*amount], result_expo).expect("Couldn't compute user deposit value").price)?)
            );
        }
        v
    };
    let total_user_deposit_value: Uint128 = Uint128::new(safe_i64_to_u128(Price::price_basket(amounts, result_expo).expect("Couldn't compute user deposit value").price)?);


    // Calculate aum
    // TODO: ensure reserve_basket_asset_info is the correct one
    let aum_result = basket.calculate_aum(&price_feeds, &basket.assets[0].info)?;
    
    // Retrieve LP token supply
    let total_share: Uint128 = query_supply(&deps.querier, basket.lp_token_address.clone())?;

    
    // Calculate share
    let share: Uint128 = if total_share.is_zero() {

        // Handle deposit into empty basket at 1:1 USD mint. First deposit gets zero fees
        total_user_deposit_value

    } else {

        // Handle deposit into nonempty basket

        // TODO: do we need to check for slippage for any reason if we use oracles? Maybe if user doesn't want to pay max bps fee?
        // Assert slippage tolerance
        // assert_slippage_tolerance(slippage_tolerance, &deposits, &pools)?;

        // exchange rate is (lp supply) / (aum)
        // here we value * rate = value * lp supply / aum, safely
        // then, we reduce fees by doing gross * ( 10000 - deposit_fee ) / 10000
        let pre_fee = total_user_deposit_value.multiply_ratio(total_share, aum_result.aum);
        let fee_bps: Uint128 = Uint128::zero();
        // {
        //     for deposit in &deposits {
        //         let deposit_value
        //     }
        // };
        
        let post_fee = pre_fee.multiply_ratio(BASIS_POINTS_PRECISION - fee_bps, BASIS_POINTS_PRECISION);
        post_fee
    };

    // TODO: I think this is where we subtract fees from share. I may be wrong.
    // Also I think first depositor is charged no fee if we do it here because they just get minted less but they own 100% of lp token.
    // Maybe we take difference and mint it to some fee wallet?



    
    // Update the pool_reserves field in each BasketAsset to reflect the amounts deposited to the contract account)
    for (i, basket_asset) in basket.assets.iter_mut().enumerate() {
        basket_asset.pool_reserves += deposits[i].amount;
    }

    // Mint LP tokens for the sender or for the receiver (if set)
    let receiver = receiver.unwrap_or_else(|| info.sender.to_string());
    messages.extend(mint_liquidity_token_message(
        deps.as_ref(),
        &basket,
        env.clone(),
        validate_addr(deps.api, &receiver)?,
        share,
    ).map_err(|_| ContractError::LpMintFailed)?);

    // Return response with attributes
    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("action", "provide_liquidity"),
        attr("sender", info.sender.as_str()),
        attr("receiver", receiver.as_str()),
        attr("assets", format!("{:?}", &assets)),
        attr("share", share.to_string()),
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



fn safe_u128_to_i64(input: u128) -> Result<i64, ContractError> {
    let output = input as i64;
    if output as u128 == input {
        return Ok(output)
    } else {
        return Err(ContractError::FailedCast)
    }
}

fn safe_i64_to_u128(input: i64) -> Result<u128, ContractError> {
    let output = input as u128;
    if output as i64 == input {
        return Ok(output)
    } else {
        return Err(ContractError::FailedCast)
    }
}



fn safe_i64_expo_to_u128(
    amount: i64,
    expo: i32,
) -> Uint128 {

    // Positive price
    assert!(amount >= 0, "amount must be non-negative");
    let amount: u128 = amount as u128;

    if expo >= 0 {

        // Deal with non-negative exponent
        let expo = expo as u32;
        Uint128::from(amount).multiply_ratio(10_u32.pow(expo), 1_u32)
    } else {

        // Deal with negative exponent
        let expo = expo.abs() as u32;
        Uint128::from(amount).multiply_ratio(1_u32, 10_u32.pow(expo))
    }

}