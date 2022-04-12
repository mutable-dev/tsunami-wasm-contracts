use crate::contract::{ 
    instantiate,
    query_basket,
 };
use crate::mock_querier::mock_dependencies;
use crate::{
    msg::*,
    state::{Basket, BasketAsset},
    asset::{Asset, AssetInfo},
};

use cosmwasm_std::testing::{mock_env, mock_info, MOCK_CONTRACT_ADDR};
use cosmwasm_std::{
    to_binary,  Addr,
    ReplyOn, SubMsg, Uint128,
    WasmMsg,
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