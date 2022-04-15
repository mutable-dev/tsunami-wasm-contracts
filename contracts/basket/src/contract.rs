use crate::{
    error::ContractError,
    msg::*,
    asset::{AssetInfo, Asset},
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
use std::cmp::max;
use pyth_sdk_terra::{PriceFeed, Price, PriceIdentifier, PriceStatus};


/// Contract name that is used for migration.
const CONTRACT_NAME: &str = "tsunami-basket";
/// Contract version that is used for migration.
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const INSTANTIATE_BASKET_REPLY_ID: u64 = 1;
const BASIS_POINTS_PRECISION: Uint128 = Uint128::new(10_000);
const FEE_IN_BASIS_POINTS: Uint128 = Uint128::new(10_030);
const FEE_RAW: Uint128 = Uint128::new(30);
const PENALTY_IN_BASIS_POINTS: Uint128 = Uint128::new(30);


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
        ExecuteMsg::Receive { msg } => receive_cw20(deps, env, info, msg),
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

//     basket.lp_token_address =
//         addr_validate_to_lower(deps.api, res.get_contract_address())?;

//     CONFIG.save(deps.storage, &basket)?;

//     Ok(Response::new().add_attribute("liquidity_token_addr", basket.lp_token_address))
// }

pub fn withdraw_liquidity(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    sender: Addr,
    amount: Uint128,
    basket_asset: BasketAsset,
) -> Result<Response, ContractError> {
    
    // Load Basket
    let basket: Basket = BASKET.load(deps.storage).unwrap();

    // Abort if not from basket lp token contract
    if info.sender != basket.lp_token_address {
        return Err(ContractError::Unauthorized {});
    }

    // TODO: encode which asset to withdraw in msg (may not be possible. may need some reworking)
    // For now, just send back luna always 
    let asset_info: AssetInfo = basket_asset.info;

    // Mock Pyth prices
    let price = PriceFeed::new(
        PriceIdentifier::new([0; 32]),
        PriceStatus::default(),
        0,
        6,
        5,
        10_000_000,
        PriceIdentifier::new([0; 32]),
        0,
        0,
        0,
        0,
        0,
        0,
        0
    );
    let refund_asset = basket.withdraw_amount(amount, &[price], asset_info)?; // get_share_in_assets(&pools, amount, total_share);

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
                return Err(ContractError::Unauthorized {});
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
) -> Result<Response, ContractError> {

    // offer_asset.assert_sent_native_token_balance(&info)?;

    // let mut basket: Basket = BASKET.load(deps.storage)?;

    // // If the asset balance is already increased, we should subtract the user deposit from the pool amount
    // let pools: Vec<Asset> = basket
    //     .pair_info
    //     .query_pools(&deps.querier, env.clone().contract.address)?
    //     .iter()
    //     .map(|p| {
    //         let mut p = p.clone();
    //         if p.info.equal(&offer_asset.info) {
    //             p.amount = p.amount.checked_sub(offer_asset.amount).unwrap();
    //         }

    //         p
    //     })
    //     .collect();

    // let offer_pool: Asset;
    // let ask_pool: Asset;

    // if offer_asset.info.equal(&pools[0].info) {
    //     offer_pool = pools[0].clone();
    //     ask_pool = pools[1].clone();
    // } else if offer_asset.info.equal(&pools[1].info) {
    //     offer_pool = pools[1].clone();
    //     ask_pool = pools[0].clone();
    // } else {
    //     return Err(ContractError::AssetMismatch {});
    // }

    // // Get fee info from the factory
    // let fee_info = query_fee_info(
    //     &deps.querier,
    //     basket.factory_addr.clone(),
    //     basket.pair_info.pair_type.clone(),
    // )?;

    // let offer_amount = offer_asset.amount;
    // let (return_amount, spread_amount, commission_amount) = compute_swap(
    //     offer_pool.amount,
    //     ask_pool.amount,
    //     offer_amount,
    //     fee_info.total_fee_rate,
    // )?;

    // // Check the max spread limit (if it was specified)
    // assert_max_spread(
    //     belief_price,
    //     max_spread,
    //     offer_amount,
    //     return_amount + commission_amount,
    //     spread_amount,
    // )?;

    // // Compute the tax for the receiving asset (if it is a native one)
    // let return_asset = Asset {
    //     info: ask_pool.info.clone(),
    //     amount: return_amount,
    // };

    // let tax_amount = return_asset.compute_tax(&deps.querier)?;
    // let receiver = to.unwrap_or_else(|| sender.clone());
    // let mut messages: Vec<CosmosMsg> =
    //     vec![return_asset.into_msg(&deps.querier, receiver.clone())?];

    // // Compute the Maker fee
    // let mut maker_fee_amount = Uint128::new(0);
    // if let Some(fee_address) = fee_info.fee_address {
    //     if let Some(f) = calculate_maker_fee(
    //         ask_pool.info.clone(),
    //         commission_amount,
    //         fee_info.maker_fee_rate,
    //     ) {
    //         messages.push(f.clone().into_msg(&deps.querier, fee_address)?);
    //         maker_fee_amount = f.amount;
    //     }
    // }

    // // Accumulate prices for the assets in the pool
    // if let Some((price0_cumulative_new, price1_cumulative_new, block_time)) =
    //     accumulate_prices(env, &basket, pools[0].amount, pools[1].amount)?
    // {
    //     basket.price0_cumulative_last = price0_cumulative_new;
    //     basket.price1_cumulative_last = price1_cumulative_new;
    //     basket.block_time_last = block_time;
    //     CONFIG.save(deps.storage, &basket)?;
    // }

    // Ok(Response::new()
    //     .add_messages(
    //         // 1. send collateral tokens from the contract to a user
    //         // 2. send inactive commission fees to the Maker ontract
    //         messages,
    //     )
    //     .add_attribute("action", "swap")
    //     .add_attribute("sender", sender.as_str())
    //     .add_attribute("receiver", receiver.as_str())
    //     .add_attribute("offer_asset", offer_asset.info.to_string())
    //     .add_attribute("ask_asset", ask_pool.info.to_string())
    //     .add_attribute("offer_amount", offer_amount.to_string())
    //     .add_attribute("return_amount", return_amount.to_string())
    //     .add_attribute("tax_amount", tax_amount.to_string())
    //     .add_attribute("spread_amount", spread_amount.to_string())
    //     .add_attribute("commission_amount", commission_amount.to_string())
    //     .add_attribute("maker_fee_amount", maker_fee_amount.to_string()))
    

    // TODO: IMPLEMENT FN AND REMOVE THIS OK
    Ok(Response::new())
}

// cases to consider
// 1. initialAmount is far from targetAmount, action increases balance slightly => high rebate
// 2. initialAmount is far from targetAmount, action increases balance largely => high rebate
// 3. initialAmount is close to targetAmount, action increases balance slightly => low rebate
// 4. initialAmount is far from targetAmount, action reduces balance slightly => high tax
// 5. initialAmount is far from targetAmount, action reduces balance largely => high tax
// 6. initialAmount is close to targetAmount, action reduces balance largely => low tax
// 7. initialAmount is above targetAmount, nextAmount is below targetAmount and vice versa
// 8. a large swap should have similar fees as the same trade split into multiple smaller swaps
/// CHECK: types here are bad, and conversions too many, need to consolidate
/// CHECK: that we are doing the correct math when calculating
/// fees that should be charged 
/// CHECK: that we are calculating available assets correctly
/// CHECK: that we should calculate the current reserves to compare against target reserves using 
/// only the available asset, relies on how AUM is calculated
pub fn calculate_fee_basis_points(
	aum: Uint128,
	basket_asset: &BasketAsset, 
	total_weight: Uint128, 
	price: Uint128,
	exponent: u32,
	new_amount: Uint128,
	increment: bool
) -> Uint128 {
	let current_reserves = basket_asset.pool_reserves;
	let initial_reserve_usd_value = (current_reserves).
		checked_mul(price).
		unwrap().
		checked_div(Uint128::new(10).pow(exponent))
		.unwrap();

	let diff_usd_value = new_amount.
		checked_mul(price).
		unwrap().
		checked_div(Uint128::new(10).pow(exponent))
		.unwrap();

	let next_reserve_usd_value = if increment { 
		initial_reserve_usd_value + diff_usd_value 
	} else { 
		max(initial_reserve_usd_value - diff_usd_value, Uint128::new(0))
	};
	
	let target_lp_usd_value = basket_asset.token_weight.
		checked_mul(aum).
		unwrap().
		checked_div(total_weight).
		unwrap();

	if target_lp_usd_value == Uint128::new(0) {
		return FEE_IN_BASIS_POINTS;
	}

	let initial_usd_from_target = if initial_reserve_usd_value > target_lp_usd_value { 
		initial_reserve_usd_value - target_lp_usd_value
	} else { target_lp_usd_value - initial_reserve_usd_value  };

	let next_usd_from_target = if next_reserve_usd_value > target_lp_usd_value { 
		next_reserve_usd_value - target_lp_usd_value
	} else { target_lp_usd_value - next_reserve_usd_value  };

	// action improves target balance
	if next_usd_from_target < initial_usd_from_target {
		let rebate_bps = (FEE_IN_BASIS_POINTS ).
			checked_sub(BASIS_POINTS_PRECISION ).
			unwrap().
			checked_mul(initial_usd_from_target).
			unwrap().
			checked_div(target_lp_usd_value ).
			unwrap();
		return if rebate_bps >= FEE_RAW  {
			BASIS_POINTS_PRECISION 
		} else { 
			FEE_IN_BASIS_POINTS - rebate_bps
		};
	} else if next_usd_from_target == initial_usd_from_target {
		return FEE_IN_BASIS_POINTS
	}

	let mut average_diff = (initial_usd_from_target + next_usd_from_target)/Uint128::new(2);
	if average_diff > target_lp_usd_value {
		average_diff = target_lp_usd_value ;
	}

	let penalty = PENALTY_IN_BASIS_POINTS * average_diff / target_lp_usd_value ;
	return FEE_IN_BASIS_POINTS + penalty
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

    // Retrieve each asset pool, and order the deposit assets in the same order as the pools
    let mut basket: Basket = BASKET.load(deps.storage)?;
    let mut pools: Vec<Asset> = basket.get_pools();
    let deposits: Vec<Uint128> = {
        let mut v = vec![];
        for pool in &pools {
            v.push(assets
                .iter()
                .find(|a| a.info.equal(&pool.info))
                .map(|a| a.amount)
                .expect("Wrong asset info is given"));
        }
        v
    };
       
    let mut messages: Vec<CosmosMsg> = vec![];
    for (i, pool) in pools.iter_mut().enumerate() {
        // If the asset is a token contract, then we need to execute a TransferFrom msg to receive assets
        if let AssetInfo::Token { contract_addr, .. } = &pool.info {
            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: contract_addr.to_string(),
                msg: to_binary(&Cw20ExecuteMsg::TransferFrom {
                    owner: info.sender.to_string(),
                    recipient: env.contract.address.to_string(),
                    amount: deposits[i],
                })?,
                funds: vec![],
            }));
        } else {
            // If the asset is native token, the pool balance is already increased
            // To calculate the total amount of deposits properly, we should subtract the user deposit from the pool
            pool.amount = pool.amount.checked_sub(deposits[i])?;
        }
    }

    // Need to check this part still
    // let total_share = query_supply(&deps.querier, config.pair_info.liquidity_token.clone())?;
    // let share = if total_share.is_zero() {
    //     // Initial share = collateral amount
    //     Uint128::new(
    //         (U256::from(deposits[0].u128()) * U256::from(deposits[1].u128()))
    //             .integer_sqrt()
    //             .as_u128(),
    //     )
    // } else {
    //     // Assert slippage tolerance
    //     assert_slippage_tolerance(slippage_tolerance, &deposits, &pools)?;

    //     // min(1, 2)
    //     // 1. sqrt(deposit_0 * exchange_rate_0_to_1 * deposit_0) * (total_share / sqrt(pool_0 * pool_1))
    //     // == deposit_0 * total_share / pool_0
    //     // 2. sqrt(deposit_1 * exchange_rate_1_to_0 * deposit_1) * (total_share / sqrt(pool_1 * pool_1))
    //     // == deposit_1 * total_share / pool_1
    //     std::cmp::min(
    //         deposits[0].multiply_ratio(total_share, pools[0].amount),
    //         deposits[1].multiply_ratio(total_share, pools[1].amount),
    //     )
    // };

    // // Mint LP tokens for the sender or for the receiver (if set)
    // let receiver = receiver.unwrap_or_else(|| info.sender.to_string());
    // messages.extend(mint_liquidity_token_message(
    //     deps.as_ref(),
    //     &config,
    //     env.clone(),
    //     addr_validate_to_lower(deps.api, receiver.as_str())?,
    //     share,
    //     auto_stake,
    // )?);

    // // Accumulate prices for the assets in the pool
    // if let Some((price0_cumulative_new, price1_cumulative_new, block_time)) =
    //     accumulate_prices(env, &config, pools[0].amount, pools[1].amount)?
    // {
    //     config.price0_cumulative_last = price0_cumulative_new;
    //     config.price1_cumulative_last = price1_cumulative_new;
    //     config.block_time_last = block_time;
    //     CONFIG.save(deps.storage, &config)?;
    // }

    // Ok(Response::new().add_messages(messages).add_attributes(vec![
    //     attr("action", "provide_liquidity"),
    //     attr("sender", info.sender.as_str()),
    //     attr("receiver", receiver.as_str()),
    //     attr("assets", format!("{}, {}", assets[0], assets[1])),
    //     attr("share", share.to_string()),
    // ]))
    Ok(Response::new())
}


pub struct AumResult {
    pub aum: Uint128,
    pub price: i64,
    pub exponent: i32,
}

// CHECK: that we should take the value of the token account as AUM and not the general reserves from the
// available asset account
pub fn calculate_aum(
	prices: &Vec<PriceFeed>, 
	basket_assets: &[&BasketAsset],
	reserve_basket_asset: &BasketAsset,
) -> Result<AumResult, ContractError> {
	let mut aum = Uint128::new(0);
	let mut precise_price = 0;
	let mut exponent =  1;
	let mut current_basket_asset: &BasketAsset = &basket_assets[0];
    let reserve_asset_info: &AssetInfo = &reserve_basket_asset.info;
    let reserve_asset_denom: String;
    match reserve_asset_info {
        AssetInfo::NativeToken{ denom } => reserve_asset_denom = denom.to_string(),
        _ => {
            return Err(ContractError::NonNativeAssetAssertion{});
        }
    }

	for (i, pyth_price) in prices.iter().enumerate() {
		current_basket_asset = &basket_assets[i];

		let price_option = pyth_price.get_current_price();
        let price: Price;
        match price_option {
            Some(price_res) => price = price_res,
            _ => return Err(ContractError::PriceFeedNotFound{})
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

		aum += current_basket_asset.pool_reserves.checked_mul(Uint128::new(price.price as u128))
            .unwrap()
			.checked_div(
				Uint128::new(10_u64.pow(price.expo.abs() as u32) as u128)
			)
			.unwrap();
	}
	Ok(AumResult{ aum, price: precise_price, exponent })
}