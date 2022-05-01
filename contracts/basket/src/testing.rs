use crate::contract::{ 
    instantiate,
    query_basket,
    calculate_fee_basis_points,
    Action, execute, LP_DECIMALS
 };
use crate::error::ContractError;
use crate::mock_querier::mock_dependencies;
use crate::state::OracleInterface;
use crate::{
    msg::*,
    state::{Basket, BasketAsset, TickerData, BASKET},
    asset::{Asset, AssetInfo},
};

use cosmwasm_std::coins;
use pyth_sdk_terra::{PriceFeed, PriceIdentifier, PriceStatus};
use cosmwasm_std::testing::{mock_env, mock_info, MOCK_CONTRACT_ADDR };
use cosmwasm_std::{
    to_binary,  Addr, Response, attr,
    ReplyOn, SubMsg, Uint128,
    WasmMsg, BalanceResponse, from_binary, BankQuery, QueryRequest, Coin,
    StdError::GenericErr, BankMsg, CosmosMsg, Binary
};
use cw20::{ MinterResponse, Cw20ReceiveMsg, Cw20ExecuteMsg };

#[test]
fn proper_initialization() {
    let mut deps = mock_dependencies(&[]);

    deps.querier.with_token_balances(&[(
        &String::from("asset0000"),
        &[(&String::from(MOCK_CONTRACT_ADDR), &Uint128::new(123u128))],
    )]);

    // luna and ust info
    let luna_info = AssetInfo::NativeToken{ denom: "luna".to_string() };
    let ust_info = AssetInfo::NativeToken{ denom: "uust".to_string() };

    let mut assets = Vec::new();
    assets.push(InstantiateAssetInfo{
        info: luna_info.clone(),
        address: Addr::unchecked("name"),
        weight: Uint128::new(1),
        min_profit_basis_points: Uint128::new(1),
        max_asset_amount: Uint128::new(1),
        is_asset_stable: true,
        is_asset_shortable: true,
        oracle: OracleInterface::from_dummy(100, 0),
        backup_oracle: OracleInterface::from_dummy(100, 0),
        ticker_data: create_ticker_data()
    });
    let msg = InstantiateMsg {
        assets: assets,
        /// Name of Basket
        name: "blue chip basket".to_string(),
        /// fee for non-stable asset perp
        tax_basis_points: Uint128::new(1),
        /// fee for stable asset perp
        stable_tax_basis_points: Uint128::new(1),
        /// base fee for mint/burning lp token
        mint_burn_basis_points: Uint128::new(1),
        /// base fee for swap
        swap_fee_basis_points: Uint128::new(1),
        /// base fee for swaping between stable assets 
        stable_swap_fee_basis_points: Uint128::new(1), 
        /// references position fees, not for funding rate, nor for getting in/out of a position
        margin_fee_basis_points: Uint128::new(1), 
        /// fee for getting liquidated, goes to liquidator in USD
        liquidation_fee_usd: Uint128::new(1),
        /// prevents gaming of oracle with hourly trades
        min_profit_time: Uint128::new(1),
        /// account that can make changes to the exchange
        admin: Addr::unchecked("name"),
        /// The token contract code ID used for the tokens in the pool
        token_code_id: 10u64,
    };

    let sender = "addr0000";
    // We can just call .unwrap() to assert this was a success
    let env = mock_env();
    let info = mock_info(sender, &[]);
    let res = instantiate(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(
        res.messages,
        vec![SubMsg {
            msg: WasmMsg::Instantiate {
                code_id: 10u64,
                msg: to_binary(&InstantiateLpMsg {
                    name: "blue chip basket-LP".to_string(),
                    symbol: "TLP".to_string(),
                    decimals: LP_DECIMALS,
                    initial_balances: vec![],
                    mint: Some(MinterResponse {
                        minter: MOCK_CONTRACT_ADDR.to_string(),
                        cap: None,
                    }),
                })
                .unwrap(),
                funds: vec![],
                admin: None,
                label: String::from("Tsunami LP Token"),
            }
            .into(),
            id: 1,
            gas_limit: None,
            reply_on: ReplyOn::Success
        },]
    );

    let basket: Basket = query_basket(deps.as_ref()).unwrap();
    assert_eq!(basket.name, "blue chip basket");
    assert_eq!(basket.assets, vec![BasketAsset{
        info: luna_info.clone(),
        token_weight: Uint128::new(1),
        min_profit_basis_points: Uint128::new(1),
        max_asset_amount: Uint128::new(1),
        stable_token: true,
        shortable_token: true,
        oracle: OracleInterface::from_dummy(100, 0),
        backup_oracle: OracleInterface::from_dummy(100, 0),
        cumulative_funding_rate: Uint128::new(0),
        global_short_size: Uint128::new(0),
        net_protocol_liabilities: Uint128::new(0),
        last_funding_time: Uint128::new(0),
        occupied_reserves: Uint128::new(0),
        available_reserves: Uint128::new(0),
        fee_reserves: Uint128::new(0),
        ticker_data: create_ticker_data()
    }]);
    assert_eq!(basket.tax_basis_points, Uint128::new(1));
    assert_eq!(basket.stable_swap_fee_basis_points, Uint128::new(1));
    assert_eq!(basket.mint_burn_basis_points, Uint128::new(1));
    assert_eq!(basket.swap_fee_basis_points, Uint128::new(1));
    assert_eq!(basket.stable_swap_fee_basis_points, Uint128::new(1));
    assert_eq!(basket.margin_fee_basis_points, Uint128::new(1));
    assert_eq!(basket.liquidation_fee_usd, Uint128::new(1));
    assert_eq!(basket.min_profit_time, Uint128::new(1));
    assert_eq!(basket.admin, Addr::unchecked("name"));
}

// Create a default instantiate msg
fn create_instantiate_msg() -> InstantiateMsg {
    InstantiateMsg {
        assets: vec![create_instantiate_asset_info()],
        name: "blue chip basket".to_string(),
        tax_basis_points: Uint128::new(1),
        stable_tax_basis_points: Uint128::new(1),
        mint_burn_basis_points: Uint128::new(1),
        swap_fee_basis_points: Uint128::new(1),
        stable_swap_fee_basis_points: Uint128::new(1), 
        margin_fee_basis_points: Uint128::new(1), 
        liquidation_fee_usd: Uint128::new(1),
        min_profit_time: Uint128::new(1),
        admin: Addr::unchecked("name"),
        token_code_id: 10u64,
    }
}

fn create_ticker_data() -> TickerData {
    return TickerData {
        testnet_address: Addr::unchecked("0x0000000000000000000000000000000000000000"),
        mainnet_address: Addr::unchecked("0x0000000000000000000000000000000000000000"),
        dummy_address: Addr::unchecked("0x0000000000000000000000000000000000000000"),
        testnet_price_feed: PriceIdentifier::from_hex(
            "0a3f000000000000000000000000000000000000000000000000000000000000",
        ).unwrap(),
        mainnet_price_feed: PriceIdentifier::from_hex(
            "0a3f000000000000000000000000000000000000000000000000000000000000",
        ).unwrap(),
        dummy_price_feed: PriceIdentifier::from_hex(
            "0a3f000000000000000000000000000000000000000000000000000000000000",
        ).unwrap(),
    }
}

/// Create a default instantiate asset info struct so we can fill in fields we're not interested in
fn create_instantiate_asset_info() -> InstantiateAssetInfo {
    InstantiateAssetInfo {
        info: AssetInfo::NativeToken{ denom: "default".to_string() },
        address: Addr::unchecked("default_addr"),
        weight: Uint128::new(1),
        min_profit_basis_points: Uint128::new(1),
        max_asset_amount: Uint128::new(100),
        is_asset_stable: true,
        is_asset_shortable: true,
        oracle: OracleInterface::from_dummy(100, 0),
        backup_oracle: OracleInterface::from_dummy(100, 0),
        ticker_data: create_ticker_data()
    }
}

#[test]
fn exploration() {
    assert_eq!(2 + 2, 4);
}

fn create_basket() -> Basket {
    let basket_asset = create_basket_asset();
    let basket_asset2 = create_basket_asset();
    let basket_asset_copy = create_basket_asset();
    Basket::new(
        vec!(basket_asset, basket_asset_copy.clone()), 
        &InstantiateMsg{
            assets: vec!(create_instantiate_asset_info()),
            name: "blue chip basket".to_string(),
            tax_basis_points: Uint128::new(1),
            stable_tax_basis_points: Uint128::new(1),
            mint_burn_basis_points: Uint128::new(1),
            swap_fee_basis_points: Uint128::new(1),
            stable_swap_fee_basis_points: Uint128::new(1),
            margin_fee_basis_points: Uint128::new(1),
            liquidation_fee_usd: Uint128::new(1),
            min_profit_time: Uint128::new(1),
            admin: Addr::unchecked("name"),
            token_code_id: 10u64,
        },
    )
}

fn create_basket_asset() -> BasketAsset {
    BasketAsset {
        info: AssetInfo::NativeToken{denom: "uluna".to_string()},
        token_weight:  Uint128::new(1),
        min_profit_basis_points:  Uint128::new(100),
        max_asset_amount:  Uint128::new(100),
        stable_token: false,
        shortable_token: false,
        cumulative_funding_rate:  Uint128::new(0),
        last_funding_time:  Uint128::new(0),
        oracle: OracleInterface::from_dummy(100, 0),
        backup_oracle: OracleInterface::from_dummy(100, 0),
        global_short_size:  Uint128::new(0),
        net_protocol_liabilities:  Uint128::new(0),
        occupied_reserves:  Uint128::new(0),
        fee_reserves: Uint128::new(0),
        available_reserves:  Uint128::new(400),
        ticker_data: create_ticker_data()
    }
}

pub fn create_price_feed(price: i64, exponent: i32) -> PriceFeed {
    PriceFeed::new(
        PriceIdentifier::new([0; 32]),
        PriceStatus::Trading,
        0,
        exponent,
        5,
        10_000_000,
        PriceIdentifier::new([0; 32]),
        price,
        0,
        0,
        0,
        0,
        0,
        0
    )
}

#[test]
fn slightly_improves_basket_add() {
    let basket_asset = create_basket_asset();
    let basket = create_basket();
    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket,
        &vec![Uint128::new(40_000)],
        &vec![Uint128::new(1_000)],
        &vec![basket_asset],
        Action::Offer,
    );
    assert_eq!(vec![Uint128::new(12)], fees);
}

#[test]
fn strongly_improves_basket_add() {
    let mut basket_asset = create_basket_asset();
    let basket = create_basket();
    basket_asset.available_reserves = Uint128::new(4);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket,
        &vec![Uint128::new(1_000)],
        &vec![Uint128::new(1_000)],
        &vec![basket_asset],
        Action::Offer,
    );
    assert_eq!(vec![Uint128::new(0)], fees);
}

#[test]
fn strongly_harms_basket_add() {
    let mut basket_asset = create_basket_asset();
    let basket = create_basket();
    basket_asset.available_reserves = Uint128::new(500);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket,
        &vec![Uint128::new(90_000)],
        &vec![Uint128::new(100_000)],
        &vec![basket_asset],
        Action::Offer,
    );
    assert_eq!(vec![Uint128::new(28)], fees);
}

#[test]
fn lightly_harms_basket_add() {
    let mut basket_asset = create_basket_asset();
    let basket = create_basket();
    basket_asset.available_reserves = Uint128::new(500);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket,
        &vec![Uint128::new(52_000)],
        &vec![Uint128::new(1_000)],
        &vec![basket_asset],
        Action::Offer,
    );
    assert_eq!(vec![Uint128::new(15)], fees);
}

#[test]
fn slightly_improves_basket_remove() {
        let mut basket_asset = create_basket_asset();
        let basket = create_basket();
        basket_asset.available_reserves = Uint128::new(550);
        let fees = calculate_fee_basis_points(
            Uint128::new(100_000),
            &basket,
            &vec![Uint128::new(60_000)],
            &vec![Uint128::new(1_000)],
            &vec![basket_asset],
            Action::Ask,
        );
        assert_eq!(vec![Uint128::new(12)], fees);
}

#[test]
fn strongly_improves_basket_remove() {
    let mut basket_asset = create_basket_asset();
    let basket = create_basket();
    basket_asset.available_reserves = Uint128::new(1000);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket,
        &vec![Uint128::new(95_000)],
        &vec![Uint128::new(10_000)],
        &vec![basket_asset],
        Action::Ask,
    );
    assert_eq!(vec![Uint128::new(1)], fees);
}

#[test]
fn strongly_harms_basket_remove() {
    let mut basket_asset = create_basket_asset();
    let basket = create_basket();
    basket_asset.available_reserves = Uint128::new(10);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket,
        &vec![Uint128::new(10_000)],
        &vec![Uint128::new(1_000)],
        &vec![basket_asset],
        Action::Ask,
    );
    assert_eq!(vec![Uint128::new(27)], fees);
}

#[test]
fn lightly_harms_basket_remove() {
    let mut basket_asset = create_basket_asset();
    let basket = create_basket();
    basket_asset.available_reserves = Uint128::new(500);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket,
        &vec![Uint128::new(48_000)],
        &vec![Uint128::new(1_000)],
        &vec![basket_asset],
        Action::Ask,
    );
    assert_eq!(vec![Uint128::new(16)], fees);
}

#[test]
fn neutral_basket_remove() {
    let mut basket_asset = create_basket_asset();
    let basket = create_basket();
    basket_asset.available_reserves = Uint128::new(550);

    let fees = calculate_fee_basis_points(
        Uint128::new(101_000),
        &basket,
        &vec![Uint128::new(51_000)],
        &vec![Uint128::new(1_000)],
        &vec![basket_asset],
        Action::Ask,
    );
    assert_eq!(vec![Uint128::new(15)], fees);
}

#[test]
fn neutral_basket_add() {
    let mut basket_asset = create_basket_asset();
    let basket = create_basket();
    basket_asset.available_reserves = Uint128::new(450);

    let fees = calculate_fee_basis_points(
        Uint128::new(99_000),
        &basket,
        &vec![Uint128::new(49_000)],
        &vec![Uint128::new(1_000)],
        &vec![basket_asset],
        Action::Offer,
    );
    assert_eq!(vec![Uint128::new(14)], fees);
}


#[test]
fn imbalanced_basket_big_double_balanced_add() {
    let mut basket_asset = create_basket_asset();
    let basket = create_basket();
    basket_asset.available_reserves = Uint128::new(450);

    let fees = calculate_fee_basis_points(
        Uint128::new(10_000),
        &basket,
        &vec![Uint128::new(9_900), Uint128::new(100)],
        &vec![Uint128::new(100_000), Uint128::new(100_000)],
        &vec![basket_asset],
        Action::Offer,
    );
    assert_eq!(vec![Uint128::new(0)], fees);
}

// #[test]
// fn test_calculate_aum_one_asset() {
//     let mut basket = create_basket();
//     let basket_asset = create_basket_asset();
//     let mut deps = mock_dependencies(&[]);
//     basket.assets[0].available_reserves = Uint128::new(450);

//     let mut price_feeds = Vec::new();
//     price_feeds.push(create_price_feed(10_000_000, 6));
//     let aum = safe_price_to_U128(basket.calculate_aum(
//         &deps.querier,
//     ).unwrap());
//     assert_eq!(Uint128::new(4500), aum_result.aum);
//     assert_eq!(6, aum_result.exponent);
//     assert_eq!(10_000_000, aum_result.price);
// }

// #[test]
// fn test_calculate_aum_two_assets() {
//     let mut basket = create_basket();
//     let basket_asset = create_basket_asset();
//     let basket_asset_copy = create_basket_asset();
//     let mut deps = mock_dependencies(&[]);
//     basket.assets[0].available_reserves = Uint128::new(450);
//     basket.assets[0].info = AssetInfo::NativeToken{denom: "ste".to_string()};
//     basket.assets.push(basket_asset);
//     basket.assets[1].available_reserves = Uint128::new(10);

//     let mut price_feeds = Vec::new();
//     price_feeds.push(create_price_feed(10_000_000, 6));
//     price_feeds.push(create_price_feed(1_000_000, 5));
//     let aum_result = basket.calculate_aum(
//         &deps.querier,
//     ).unwrap();
//     assert_eq!(Uint128::new(4600), aum_result.aum);
//     assert_eq!(5, aum_result.exponent);
//     assert_eq!(1_000_000, aum_result.price);
// }

//////////////////////////////////////////////////////////////////////
/// Tests for native asset deposits through the contract interface ///
//////////////////////////////////////////////////////////////////////

/// Instantiate an LP with two assets and make an initial deposit with just one asset
#[test]
fn single_asset_deposit() {
    use crate::state::BASKET;
    let mut deps = mock_dependencies(&[]);
    deps.querier.with_token_balances(&[
        (
            &String::from("lp-token"),
            &[(
                &String::from(MOCK_CONTRACT_ADDR),
                &Uint128::from(0_u32),
            )],
        ),
    ]);

    // luna and ust info
    let luna_info = AssetInfo::NativeToken{ denom: "luna".to_string() };
    let ust_info = AssetInfo::NativeToken{ denom: "ust".to_string() };

    let mut assets = Vec::new();
    assets.push(InstantiateAssetInfo {
        info: luna_info.clone(),
        address: Addr::unchecked("luna_addr"),
        ..create_instantiate_asset_info()
    });
    assets.push(InstantiateAssetInfo {
        info: ust_info.clone(),
        address: Addr::unchecked("ust_addr"),
        oracle: OracleInterface::from_dummy(1_000_000, -6),
        ..create_instantiate_asset_info()
    });

    let msg = InstantiateMsg {
        assets: assets,
        ..create_instantiate_msg()
    };

    let sender = "addr0000";
    let info = mock_info(sender, &[]);
    let env = mock_env();
    let _res = instantiate(deps.as_mut(), env, info, msg).unwrap();

    let mut basket: Basket = query_basket(deps.as_ref()).unwrap();
    basket.lp_token_address = Addr::unchecked("lp-token");
    BASKET.save(deps.as_mut().storage, &basket);

    let depositor = mock_info("first_depositor", &coins(10_000_000, "luna"));
    let deposit_asset = Asset { info: luna_info.clone(), amount: Uint128::new(10_000_000) };
    let deposit_msg = ExecuteMsg::DepositLiquidity { 
        assets: vec!(deposit_asset),
        slippage_tolerance: None,
        receiver: None
    };

    let deposit_res = execute(deps.as_mut(), mock_env(), depositor, deposit_msg).unwrap();
   // assert_eq!(deposit_res.messages.len(), 1);

    let basket: Basket = query_basket(deps.as_ref()).unwrap();
    assert_eq!(basket.assets[0].available_reserves, Uint128::new(10_000_000));
}

#[ignore = "Multi-asset deposits are not yet implemented"]
#[test]
fn multi_asset_deposit() {
    todo!("Wait until multi-asset deposits are implemented");
    let mut deps = cosmwasm_std::testing::mock_dependencies(&[]);

    // luna and ust info
    let luna_info = AssetInfo::NativeToken{ denom: "luna".to_string() };
    let ust_info = AssetInfo::NativeToken{ denom: "ust".to_string() };

    let mut assets = Vec::new();
    assets.push(InstantiateAssetInfo {
        info: luna_info.clone(),
        address: Addr::unchecked("luna_addr"),
        oracle: OracleInterface::from_dummy(100_000_000, -6),
        ..create_instantiate_asset_info()
    });
    assets.push(InstantiateAssetInfo {
        info: ust_info.clone(),
        address: Addr::unchecked("ust_addr"),
        oracle: OracleInterface::from_dummy(1_000_000, -6),
        ..create_instantiate_asset_info()
    });

    let msg = InstantiateMsg {
        assets: assets,
        ..create_instantiate_msg()
    };

    let sender = "addr0000";
    let info = mock_info(sender, &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    let basket: Basket = query_basket(deps.as_ref()).unwrap();

    let luna_deposit_amount = 10_000_000;
    let ust_deposit_amount = 10_000_000;
    let deposit_funds = [
        Coin { denom: "luna".to_string(), amount: Uint128::new(luna_deposit_amount) },
        Coin { denom: "ust".to_string(), amount: Uint128::new(ust_deposit_amount) },
    ];
    
    // TODO: Correct this code when multi-asset deposits are implemented
    // let depositor = mock_info("first_depositor", &deposit_funds);
    // let deposit_assets = vec![
    //     Asset { info: luna_info.clone(), amount: Uint128::new(luna_deposit_amount) },
    //     Asset { info: ust_info.clone(), amount: Uint128::new(ust_deposit_amount)}
    // ];
    // let deposit_msg = ExecuteMsg::DepositLiquidity { 
    //     assets: deposit_assets,
    //     slippage_tolerance: None, 
    //     receiver: None
    // };

    // let _deposit_res = execute(deps.as_mut(), mock_env(), depositor, deposit_msg).unwrap();

    // Assert that the deposited tokens end up in the possession of the contract address
    let luna_response: BalanceResponse = from_binary(&deps.querier.handle_query(&QueryRequest::Bank(
        BankQuery::Balance {
            address: MOCK_CONTRACT_ADDR.to_string(),
            denom: "luna".to_string()
        })
    ).unwrap().unwrap()).unwrap();

    let ust_response: BalanceResponse = from_binary(&deps.querier.handle_query(&QueryRequest::Bank(
        BankQuery::Balance {
            address: MOCK_CONTRACT_ADDR.to_string(),
            denom: "ust".to_string()
        })
    ).unwrap().unwrap()).unwrap();

    let contract_balance_luna = luna_response.amount;
    let contract_balance_ust = ust_response.amount;
    assert_eq!("luna", contract_balance_luna.denom);
    assert_eq!("ust", contract_balance_ust.denom);
    assert_eq!(Uint128::new(luna_deposit_amount), contract_balance_luna.amount);
    assert_eq!(Uint128::new(ust_deposit_amount), contract_balance_ust.amount);

    // Assert that the deposited amounts match with the pool reserves data in the basket
    assert_eq!(contract_balance_luna.amount, query_basket(deps.as_ref()).unwrap().assets[0].available_reserves);
    assert_eq!(contract_balance_ust.amount, query_basket(deps.as_ref()).unwrap().assets[1].available_reserves);

    // Assert that the depositor receives LP tokens in return
    let lp_token_addr = query_basket(deps.as_ref()).unwrap().lp_token_address;
    let response: BalanceResponse = from_binary(&deps.querier.handle_query(&QueryRequest::Bank(
        BankQuery::Balance {
            address: "first_depositor".to_string(),
            denom: lp_token_addr.to_string()
        })
    ).unwrap().unwrap()).unwrap();

    let depositor_balance_lp_token = response.amount;
    assert_eq!(lp_token_addr, depositor_balance_lp_token.denom);
    assert_eq!(true, depositor_balance_lp_token.amount > Uint128::new(0)); // TODO figure what the exact amount should be and check it
}

/// Make an initial deposit and then a subsequent deposit
/// Check that the resulting pool reserves are the sum of the two deposits and match the contract balance
/// Check that the second deposit has fees subtracted from the LP tokens they receive
/// For later: check that the correct amount of fees are taken
#[test]
fn multiple_deposits_and_swap_and_withdraw() {
    use crate::state::BASKET;
    let mut deps = mock_dependencies(&[]);
    deps.querier.with_token_balances(&[
        (
            &String::from("lp-token"),
            &[(
                &String::from(MOCK_CONTRACT_ADDR),
                &Uint128::from(0_u32),
            )],
        ),
    ]);

    // luna and ust info
    let luna_info = AssetInfo::NativeToken{ denom: "luna".to_string() };
    let ust_info = AssetInfo::NativeToken{ denom: "ust".to_string() };

    let mut assets = Vec::new();
    assets.push(InstantiateAssetInfo {
        info: luna_info.clone(),
        address: Addr::unchecked("luna_addr"),
        oracle: OracleInterface::from_dummy(100_000_000, -6),
        ..create_instantiate_asset_info()
    });
    assets.push(InstantiateAssetInfo {
        info: ust_info.clone(),
        address: Addr::unchecked("ust_addr"),
        oracle: OracleInterface::from_dummy(1_000_000, -6),
        ..create_instantiate_asset_info()
    });

    let msg = InstantiateMsg {
        assets: assets,
        ..create_instantiate_msg()
    };

    let sender = "addr0000";
    let info = mock_info(sender, &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info.clone(), msg).unwrap();

    let mut basket: Basket = query_basket(deps.as_ref()).unwrap();
    basket.lp_token_address = Addr::unchecked("lp-token");
    BASKET.save(deps.as_mut().storage, &basket);

    let luna_amount1 = 10_000_000;
    let luna_amount2 = 1_000_000;
    let depositor1 = mock_info("first_depositor", &coins(luna_amount1, "luna"));
    let depositor2 = mock_info("second_depositor", &coins(luna_amount2, "luna"));
    let deposit_asset1 = Asset { info: luna_info.clone(), amount: Uint128::new(luna_amount1) };
    let deposit_asset2 = Asset { info: luna_info.clone(), amount: Uint128::new(luna_amount2) };

    let deposit_msg1 = ExecuteMsg::DepositLiquidity { 
        assets: vec!(deposit_asset1.clone()),
        slippage_tolerance: None, 
        receiver: None
    };
    let deposit_res1 = execute(deps.as_mut(), mock_env(), depositor1.clone(), deposit_msg1).unwrap();
    
    let expected_lp_tokens = "1000000000000";
    let expected_attributes = vec![
        attr("action", "provide_liquidity"),
        attr("sender", depositor1.sender.clone().as_str()),
        attr("receiver", depositor1.sender.clone().as_str()),
        attr("offer_asset", format!("{:?}", &[deposit_asset1])),
        attr("tokens_to_mint", expected_lp_tokens),
    ];
    for i in 0..expected_attributes.len() {
        let actual_attribute = deposit_res1.attributes[i].clone();
        let expected_attribute = expected_attributes[i].clone();
        assert_eq!(actual_attribute, expected_attribute);
    }

    let deposit_msg2 = ExecuteMsg::DepositLiquidity { 
        assets: vec!(deposit_asset2.clone()),
        slippage_tolerance: None, 
        receiver: None
    };

    let deposit_res2 = execute(deps.as_mut(), mock_env(), depositor2.clone(), deposit_msg2).unwrap();
    let expected_lp_tokens = "100000000000";
    let expected_attributes = vec![
        attr("action", "provide_liquidity"),
        attr("sender", depositor2.sender.clone().as_str()),
        attr("receiver", depositor2.sender.clone().as_str()),
        attr("offer_asset", format!("{:?}", &[deposit_asset2.clone()])),
        attr("tokens_to_mint", expected_lp_tokens),
    ];
    for i in 0..expected_attributes.len() {
        let actual_attribute = deposit_res2.attributes[i].clone();
        let expected_attribute = expected_attributes[i].clone();
        assert_eq!(actual_attribute, expected_attribute);
    }


    // swap_res.messages[0].msg
    match &deposit_res2.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute{contract_addr, msg, funds}) => {
            assert_eq!(contract_addr, "lp-token");
        },
        _ => panic!("Expected BankMsg"),
    }

    let basket: Basket = query_basket(deps.as_ref()).unwrap();
    assert_eq!(basket.assets[0].available_reserves, Uint128::new(11_000_000));
    assert_eq!(basket.assets[1].available_reserves, Uint128::new(0));

    let swap = ExecuteMsg::Swap { 
        sender: Addr::unchecked(sender),
        offer_asset: Asset { info: ust_info.clone(), amount: Uint128::new(10_000_000) },
        ask_asset: luna_info.clone(),
        to: None,
        max_spread: None,
        belief_price: None,
    };
        
    let swapper = mock_info("first_depositor", &coins(10_000_000, "ust"));
    let swap_res = execute(deps.as_mut(), mock_env(), swapper, swap).unwrap();

    let swap_offer_asset = &swap_res.attributes[3].value;
    let swap_ask_asset = &swap_res.attributes[4].value;
    let swap_offer_amount = &swap_res.attributes[5].value;
    let swap_return_amount = &swap_res.attributes[6].value;
    let offer_bps = &swap_res.attributes[7].value;
    let ask_bps =  &swap_res.attributes[8].value;
    assert_eq!(swap_offer_asset, "ust");
    assert_eq!(swap_ask_asset, "luna");
    assert_eq!(swap_offer_amount, "10000000");
    assert_eq!(swap_return_amount, "100000");
    assert_eq!(offer_bps, "0");
    assert_eq!(ask_bps, "0");


    // swap_res.messages[0].msg
    match &swap_res.messages[0].msg {
        CosmosMsg::Bank(BankMsg::Send{to_address, amount}) => {
            assert_eq!(amount[0].amount, Uint128::new(100_000));
            assert_eq!(to_address, sender);
        },
        _ => panic!("Expected BankMsg"),
    }

    let basket: Basket = query_basket(deps.as_ref()).unwrap();
    assert_eq!(basket.assets[0].available_reserves, Uint128::new(10_900_000));
    assert_eq!(basket.assets[1].available_reserves, Uint128::new(10_000_000));

    let withdraw = ExecuteMsg::Receive { 
        msg: Cw20ReceiveMsg {
            amount: Uint128::new(100_000),
            sender: sender.to_string(),
            msg: Binary::from_base64("").unwrap(),
        }
    };
        
    let withdrawer = mock_info("first_depositor", &coins(10_000_000, "ust"));
    let withdraw_res = execute(deps.as_mut(), mock_env(), withdrawer, withdraw).unwrap();

    let withdraw_redemption_asset = &withdraw_res.attributes[3].value;
    let withdraw_fee_bps = &withdraw_res.attributes[4].value;
    assert_eq!(withdraw_redemption_asset, "0");
    assert_eq!(withdraw_fee_bps, "0");

    // swap_res.messages[0].msg
    match &withdraw_res.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute{contract_addr, msg, funds}) => {
            assert_eq!(contract_addr, "lp-token");
            assert_eq!(msg.clone(), to_binary(&Cw20ExecuteMsg::Burn { amount: Uint128::new(100) }).unwrap());
        },
        _ => panic!("Expected BankMsg"),
    }

}

/// Check that a user trying to send a deposit without transferring the appropriate funds
#[test]
fn try_deposit_insufficient_funds() {
    let mut deps = cosmwasm_std::testing::mock_dependencies(&[]);

    // luna and ust info
    let luna_info = AssetInfo::NativeToken{ denom: "luna".to_string() };
    let ust_info = AssetInfo::NativeToken{ denom: "ust".to_string() };

    let mut assets = Vec::new();
    assets.push(InstantiateAssetInfo {
        info: luna_info.clone(),
        address: Addr::unchecked("luna_addr"),
        ..create_instantiate_asset_info()
    });
    assets.push(InstantiateAssetInfo {
        info: ust_info.clone(),
        address: Addr::unchecked("ust_addr"),
        ..create_instantiate_asset_info()
    });

    let msg = InstantiateMsg {
        assets: assets,
        ..create_instantiate_msg()
    };

    let sender = "addr0000";
    let info = mock_info(sender, &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    let basket: Basket = query_basket(deps.as_ref()).unwrap();

    let luna_amount = 10;

     // The depositor will send a message to deposit 10 luna, but provide only 5 luna in the message
    let depositor = mock_info("first_depositor", &coins(luna_amount - 5, "luna"));
    let deposit_asset = Asset { info: luna_info.clone(), amount: Uint128::new(10) };
    let deposit_msg = ExecuteMsg::DepositLiquidity { 
        assets: vec!(deposit_asset),
        slippage_tolerance: None, 
        receiver: None
    };

    let deposit_res = execute(deps.as_mut(), mock_env(), depositor, deposit_msg);
    match deposit_res {
        Err(ContractError::Std(GenericErr { msg: _ })) => {},
        x => {
            panic!("Error should have been returned due to token balance mismatch between deposit argument and transferred amount, {:?} was returned instead", x);
        }
    }
}

/// Check that a deposit that exceeds the pool reserve limit for a basket asset fails
#[ignore = "Deposit is currently not checked for exceeding the pool reserve limit"]
#[test]
fn try_deposit_exceeding_limit() {
    let mut deps = mock_dependencies(&[]);

    // luna and ust info
    let luna_info = AssetInfo::NativeToken{ denom: "luna".to_string() };
    
    // Make the maximum asset amount of luna 10
    let mut assets = Vec::new();
    assets.push(InstantiateAssetInfo {
        info: luna_info.clone(),
        address: Addr::unchecked("luna_addr"),
        max_asset_amount: Uint128::new(10),
        ..create_instantiate_asset_info()
    });

    let msg = InstantiateMsg {
        assets: assets,
        ..create_instantiate_msg()
    };

    let sender = "addr0000";
    let info = mock_info(sender, &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    let basket: Basket = query_basket(deps.as_ref()).unwrap();

    let depositor = mock_info("first_depositor", &coins(11, "luna"));
    let deposit_asset = Asset { info: luna_info.clone(), amount: Uint128::new(11) };
    let deposit_msg = ExecuteMsg::DepositLiquidity { 
        assets: vec!(deposit_asset),
        slippage_tolerance: None, 
        receiver: None
    };

    let deposit_res = execute(deps.as_mut(), mock_env(), depositor, deposit_msg);
    match deposit_res {
        Err(ContractError::DepositLimitExceeded) => {},
        x => {
            panic!("Err(DepositLimitExceeded) should have been returned, {:?} was returned instead", x);
        }
    }
}

#[ignore = "we don't implement the whitelist yet"]
/// Check that depositing an asset the basket wasn't initialized with fails
#[test]
fn try_deposit_unwhitelisted_asset() {
    let mut deps = mock_dependencies(&[]);

    // luna and ust info
    let luna_info = AssetInfo::NativeToken{ denom: "luna".to_string() };
    
    // Make the maximum asset amount of luna 10
    let mut assets = Vec::new();
    assets.push(InstantiateAssetInfo {
        info: luna_info.clone(),
        address: Addr::unchecked("luna_addr"),
        ..create_instantiate_asset_info()
    });

    let msg = InstantiateMsg {
        assets: assets,
        ..create_instantiate_msg()
    };

    let sender = "addr0000";
    let info = mock_info(sender, &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    let basket: Basket = query_basket(deps.as_ref()).unwrap();

    let random_asset_info = AssetInfo::NativeToken{ denom: "random_asset".to_string() };
    let depositor = mock_info("first_depositor", &coins(1, "random_asset"));
    let deposit_asset = Asset { info: random_asset_info.clone(), amount: Uint128::new(1) };
    let deposit_msg = ExecuteMsg::DepositLiquidity { 
        assets: vec!(deposit_asset),
        slippage_tolerance: None, 
        receiver: None
    };

    let deposit_res = execute(deps.as_mut(), mock_env(), depositor, deposit_msg);
    match deposit_res {
        Err(ContractError::AssetNotInBasket) => {},
        x => {
            panic!("Err(AssetNotInBasket) should have been returned, {:?} was returned instead", x);
        }
    }
}