#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult, Addr};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::msg::{OwnerResponse, CountResponse, ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{State, STATE};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:counter";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let state = State {
        count: msg.count,
        owner: info.sender.clone(),
    };
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("owner", info.sender)
        .add_attribute("count", msg.count.to_string()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Swap { token_to_swap_from, token_to_swap_to } => try_swap(deps, info, token_to_swap_from, token_to_swap_to),
        ExecuteMsg::Mint { token_to_deposit } => try_mint(deps, info, token_to_deposit),
        ExecuteMsg::Burn { token_to_burn } => try_burn(deps, info, token_to_burn),
    }
}

pub fn try_swap(
    deps: DepsMut, 
    info: MessageInfo, 
    token_to_swap_from: String,
    token_to_swap_to: String,
) -> Result<Response, ContractError> {
    Ok(Response::new().add_attribute("method", "swap"))
}

pub fn try_mint(deps: DepsMut, info: MessageInfo, token_to_deposit: String) -> Result<Response, ContractError> {
    Ok(Response::new().add_attribute("method", "mint"))
}

pub fn try_burn( deps: DepsMut, info: MessageInfo, token_to_burn: String) -> Result<Response, ContractError> {
    Ok(Response::new().add_attribute("method", "burn"))
}

// #[cfg_attr(not(feature = "library"), entry_point)]
// pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
//     match msg {
//         QueryMsg::GetCount {} => to_binary(&query_count(deps)?),
//         QueryMsg::GetOwner {} => to_binary(&query_owner(deps)?),
//     }
// }

// fn query_count(deps: Deps) -> StdResult<CountResponse> {
//     let state = STATE.load(deps.storage)?;
//     Ok(CountResponse { count: state.count })
// }

// fn query_owner(deps: Deps) -> StdResult<OwnerResponse> {
//     let state = STATE.load(deps.storage)?;
//     Ok(OwnerResponse { owner: state.owner.to_string() })
// }

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coins, from_binary};

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies(&[]);

        let msg = InstantiateMsg { count: 17 };
        let info = mock_info("creator", &coins(1000, "earth"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());
    }

    #[test]
    fn mint_burn_swap_mocks() {
        let mut deps = mock_dependencies(&coins(2, "token"));

        let msg = InstantiateMsg { count: 17 };
        let info = mock_info("creator", &coins(2, "token"));
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        let info = mock_info("creator", &coins(2, "token"));
        let msg = ExecuteMsg::Swap { token_to_swap_from: "UST".to_string(), token_to_swap_to: "LUNA".to_string() };
        let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!("swap".to_string(), _res.attributes[0].value);

        let info = mock_info("creator", &coins(2, "token"));
        let msg = ExecuteMsg::Mint { token_to_deposit: "UST".to_string() };
        let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!("mint".to_string(), _res.attributes[0].value);

        let info = mock_info("creator", &coins(2, "token"));
        let msg = ExecuteMsg::Burn { token_to_burn: "UST".to_string()};
        let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!("burn".to_string(), _res.attributes[0].value);
    }
}
