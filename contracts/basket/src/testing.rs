use crate::contract::{ 
    instantiate,
    query_basket,
    calculate_fee_basis_points, execute,
 };
use crate::mock_querier::mock_dependencies;
use crate::{
    msg::*,
    state::{Basket, BasketAsset},
    asset::{Asset, AssetInfo},
};

use pyth_sdk_terra::{PriceFeed, Price, PriceIdentifier, PriceStatus};
use cosmwasm_std::testing::{mock_env, mock_info, MOCK_CONTRACT_ADDR, MockApi, MockQuerier};
use cosmwasm_std::{
    to_binary,  Addr, BankQuery,
    ReplyOn, SubMsg, Uint128,
    WasmMsg, coins, Api, QueryRequest, 
    from_binary, BalanceResponse, Deps, 
    OwnedDeps, MemoryStorage,
};
use cw20::{ MinterResponse};

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
        oracle_address: Addr::unchecked("oracle"),
        backup_oracle_address: Addr::unchecked("backup_oracle"),
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
        /// cache the total weights of the assets	
        total_weights: Uint128::new(1),
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
                    symbol: "NLP".to_string(),
                    decimals: 6,
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
        oracle_address: Addr::unchecked("oracle"),
        backup_oracle_address: Addr::unchecked("backup_oracle"),
        cumulative_funding_rate: Uint128::new(0),
        global_short_size: Uint128::new(0),
        net_protocol_liabilities: Uint128::new(0),
        last_funding_time: Uint128::new(0),
        occupied_reserves: Uint128::new(0),
        pool_reserves: Uint128::new(0),
        fee_reserves: Uint128::new(0),
    }]);
    assert_eq!(basket.tax_basis_points, Uint128::new(1));
    assert_eq!(basket.stable_swap_fee_basis_points, Uint128::new(1));
    assert_eq!(basket.mint_burn_basis_points, Uint128::new(1));
    assert_eq!(basket.swap_fee_basis_points, Uint128::new(1));
    assert_eq!(basket.stable_swap_fee_basis_points, Uint128::new(1));
    assert_eq!(basket.margin_fee_basis_points, Uint128::new(1));
    assert_eq!(basket.liquidation_fee_usd, Uint128::new(1));
    assert_eq!(basket.min_profit_time, Uint128::new(1));
    assert_eq!(basket.total_weights, Uint128::new(1));
    assert_eq!(basket.admin, Addr::unchecked("name"));
}

/// Create a default instantiate asset info struct so we can fill in fields we're not interested in
fn create_instantiate_asset_info() -> InstantiateAssetInfo {
    InstantiateAssetInfo{
        info: AssetInfo::NativeToken{ denom: "default".to_string() },
        address: Addr::unchecked("default_addr"),
        weight: Uint128::new(1),
        min_profit_basis_points: Uint128::new(1),
        max_asset_amount: Uint128::new(100),
        is_asset_stable: true,
        is_asset_shortable: true,
        oracle_address: Addr::unchecked("oracle"),
        backup_oracle_address: Addr::unchecked("backup_oracle"),
    }
}

fn create_basket() -> Basket {
    let basket_asset = create_basket_asset();
    let basket_asset_copy = create_basket_asset();
    Basket::new(
        vec!(basket_asset), 
        &InstantiateMsg{
            assets: vec!(
                InstantiateAssetInfo{
                    info: basket_asset_copy.info.clone(),
                    address: Addr::unchecked("name"),
                    weight: Uint128::new(1),
                    min_profit_basis_points: Uint128::new(1),
                    max_asset_amount: Uint128::new(1),
                    is_asset_stable: true,
                    is_asset_shortable: true,
                    oracle_address: Addr::unchecked("oracle"),
                    backup_oracle_address: Addr::unchecked("backup_oracle"),
                }
            ),
            name: "blue chip basket".to_string(),
            tax_basis_points: Uint128::new(1),
            stable_tax_basis_points: Uint128::new(1),
            mint_burn_basis_points: Uint128::new(1),
            swap_fee_basis_points: Uint128::new(1),
            stable_swap_fee_basis_points: Uint128::new(1),
            margin_fee_basis_points: Uint128::new(1),
            liquidation_fee_usd: Uint128::new(1),
            min_profit_time: Uint128::new(1),
            total_weights: Uint128::new(1),
            admin: Addr::unchecked("name"),
            token_code_id: 10u64,
        },
    )
}

fn create_basket_asset() -> BasketAsset {
    BasketAsset {
        info: AssetInfo::NativeToken{denom: "uluna".to_string()},
        token_weight:  Uint128::new(5),
        min_profit_basis_points:  Uint128::new(100),
        max_asset_amount:  Uint128::new(100),
        stable_token: false,
        shortable_token: false,
        cumulative_funding_rate:  Uint128::new(0),
        last_funding_time:  Uint128::new(0),
        oracle_address: Addr::unchecked("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS"),
        backup_oracle_address: Addr::unchecked("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS"),
        global_short_size:  Uint128::new(0),
        net_protocol_liabilities:  Uint128::new(0),
        occupied_reserves:  Uint128::new(0),
        fee_reserves: Uint128::new(0),
        pool_reserves:  Uint128::new(400)
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
    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket_asset,
        Uint128::new(10),
        Uint128::new(100_0000),
        4,
        Uint128::new(100),
        true
    );
    assert_eq!(Uint128::new(12), fees);
}

#[test]
fn strongly_improves_basket_add() {
    let basket_asset = &mut create_basket_asset();
    basket_asset.pool_reserves = Uint128::new(4);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket_asset,
        Uint128::new(10),
        Uint128::new(1_000_000),
        4,
        Uint128::new(100),
        true
    );
    assert_eq!(Uint128::new(01), fees);
}

#[test]
fn strongly_harms_basket_add() {
    let basket_asset = &mut create_basket_asset();
    basket_asset.pool_reserves = Uint128::new(500);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket_asset,
        Uint128::new(10),
        Uint128::new(1_000_000),
        4,
        Uint128::new(10000),
        true
    );
    assert_eq!(Uint128::new(30), fees);
}

#[test]
fn lightly_harms_basket_add() {
    let basket_asset = &mut create_basket_asset();
    basket_asset.pool_reserves = Uint128::new(500);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket_asset,
        Uint128::new(10),
        Uint128::new(100_0000),
        4,
        Uint128::new(50),
        true
    );
    assert_eq!(Uint128::new(15), fees);
}

#[test]
fn slightly_improves_basket_remove() {
        let basket_asset = &mut create_basket_asset();
        basket_asset.pool_reserves = Uint128::new(550);
        let fees = calculate_fee_basis_points(
            Uint128::new(100_000),
            &basket_asset,
            Uint128::new(10),
            Uint128::new(100_0000),
            4,
            Uint128::new(10),
            false
        );
        assert_eq!(Uint128::new(14), fees);
}

#[test]
fn strongly_improves_basket_remove() {
    let basket_asset = &mut create_basket_asset();
    basket_asset.pool_reserves = Uint128::new(1000);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket_asset,
        Uint128::new(10),
        Uint128::new(100_0000),
        4,
        Uint128::new(100),
        false
    );
    assert_eq!(Uint128::new(00), fees);
}

#[test]
fn strongly_harms_basket_remove() {
    let basket_asset = &mut create_basket_asset();
    basket_asset.pool_reserves = Uint128::new(10);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket_asset,
        Uint128::new(10),
        Uint128::new(100_0000),
        4,
        Uint128::new(5),
        false
    );
    assert_eq!(Uint128::new(29), fees);
}

#[test]
fn lightly_harms_basket_remove() {
    let basket_asset = &mut create_basket_asset();
    basket_asset.pool_reserves = Uint128::new(500);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket_asset,
        Uint128::new(10),
        Uint128::new(100_0000),
        4,
        Uint128::new(50),
        false
    );
    assert_eq!(Uint128::new(15), fees);
}

#[test]
fn neutral_basket_remove() {
    let basket_asset = &mut create_basket_asset();
    basket_asset.pool_reserves = Uint128::new(550);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket_asset,
        Uint128::new(10),
        Uint128::new(100_0000),
        4,
        Uint128::new(100),
        false
    );
    assert_eq!(Uint128::new(15), fees);
}

#[test]
fn neutral_basket_add() {
    let basket_asset = &mut create_basket_asset();
    basket_asset.pool_reserves = Uint128::new(450);

    let fees = calculate_fee_basis_points(
        Uint128::new(100_000),
        &basket_asset,
        Uint128::new(10),
        Uint128::new(100_0000),
        4,
        Uint128::new(100),
        true
    );
    assert_eq!(Uint128::new(15), fees);
}

#[test]
fn test_calculate_aum_one_asset() {
    let mut basket = create_basket();
    let basket_asset = create_basket_asset();
    basket.assets[0].pool_reserves = Uint128::new(450);

    let mut price_feeds = Vec::new();
    price_feeds.push(create_price_feed(10_000_000, 6));
    let aum_result  = basket.calculate_aum(
        &price_feeds,
        &basket_asset.info
    ).unwrap();
    assert_eq!(Uint128::new(4500), aum_result.aum);
    assert_eq!(6, aum_result.exponent);
    assert_eq!(10_000_000, aum_result.price);
}

#[test]
fn test_calculate_aum_two_assets() {
    let mut basket = create_basket();
    let basket_asset = create_basket_asset();
    let basket_asset_copy = create_basket_asset();
    basket.assets[0].pool_reserves = Uint128::new(450);
    basket.assets[0].info = AssetInfo::NativeToken{denom: "ste".to_string()};
    basket.assets.push(basket_asset);
    basket.assets[1].pool_reserves = Uint128::new(10);

    let mut price_feeds = Vec::new();
    price_feeds.push(create_price_feed(10_000_000, 6));
    price_feeds.push(create_price_feed(1_000_000, 5));
    let aum_result  = basket.calculate_aum(
        &price_feeds,
        &basket_asset_copy.info
    ).unwrap();
    assert_eq!(Uint128::new(4600), aum_result.aum);
    assert_eq!(5, aum_result.exponent);
    assert_eq!(1_000_000, aum_result.price);
}

/// Test the contract interface for native asset deposits to the LP

/// Instantiate an LP with one asset and make an initial deposit with one asset
#[test]
fn single_asset_deposit() {
    //let mut deps = mock_dependencies(&[]);
    let mut deps = cosmwasm_std::testing::mock_dependencies(&[]);
    // deps.querier.with_token_balances(&[(
    //     &String::from("asset0000"),
    //     &[(&String::from(MOCK_CONTRACT_ADDR), &Uint128::new(123u128))],
    // )]);

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
        name: "blue chip basket".to_string(),
        tax_basis_points: Uint128::new(1),
        stable_tax_basis_points: Uint128::new(1),
        mint_burn_basis_points: Uint128::new(1),
        swap_fee_basis_points: Uint128::new(1),
        stable_swap_fee_basis_points: Uint128::new(1), 
        margin_fee_basis_points: Uint128::new(1), 
        liquidation_fee_usd: Uint128::new(1),
        min_profit_time: Uint128::new(1),
        total_weights: Uint128::new(1),
        admin: Addr::unchecked("name"),
        token_code_id: 10u64,
    };

    let sender = "addr0000";
    let info = mock_info(sender, &[]);
    let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

    let basket: Basket = query_basket(deps.as_ref()).unwrap();
    println!("{}", basket.assets[0].pool_reserves);

    let depositor = mock_info("first_depositor", &coins(10, "luna"));
    let deposit_assets = vec![
        Asset { info: luna_info.clone(), amount: Uint128::new(10) },
    ];
    let deposit_msg = ExecuteMsg::DepositLiquidity { 
        assets: deposit_assets,
        slippage_tolerance: None, 
        receiver: None
    };

    let _deposit_res = execute(deps.as_mut(), mock_env(), depositor, deposit_msg).unwrap();

    // Assert that the deposited tokens end up in the possession of the contract address
    let response: BalanceResponse = from_binary(&deps.querier.handle_query(&QueryRequest::Bank(
        BankQuery::Balance {
            address: MOCK_CONTRACT_ADDR.to_string(),
            denom: "luna".to_string()
        })
    ).unwrap().unwrap()).unwrap();

    let contract_balance_luna = response.amount;
    assert_eq!("luna", contract_balance_luna.denom);
    assert_eq!(Uint128::new(10), contract_balance_luna.amount);

    // Assert that this deposited amount matches with the data stored in the basket (the first deposit should be feeless)
    assert_eq!(contract_balance_luna.amount, query_basket(deps.as_ref()).unwrap().assets[0].pool_reserves);
    
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

