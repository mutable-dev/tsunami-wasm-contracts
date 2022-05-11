use cosmwasm_std::{OverflowError, StdError};
use thiserror::Error;

/// ## Description
/// This enum describes pair contract errors!
#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Operation non supported")]
    NonSupported,

    #[error("Event of zero transfer")]
    InvalidZeroAmount,

    #[error("Operation exceeds max spread limit")]
    MaxSpreadAssertion,

    #[error("Provided spread amount exceeds allowed limit")]
    AllowedSpreadAssertion,

    #[error("Operation exceeds max splippage tolerance")]
    MaxSlippageAssertion,

    #[error("Doubling assets in asset infos")]
    DoublingAssets,

    #[error("Asset mismatch between the requested and the stored asset in contract")]
    AssetMismatch,

    #[error("Pair type mismatch. Check factory pair configs")]
    PairTypeMismatch,

    #[error("Duplicate asset provided")]
    DuplicateAssetAssertion,

    #[error("Unsupported non-native asset provided")]
    NonNativeAssetAssertion,

    #[error("Unable to retrieve pyth price")]
    OracleQueryFailed,

    #[error("Failed to cast between types safely")]
    FailedCast,

    #[error("Failed to mint lp token")]
    LpMintFailed,

    #[error("The asset the user is asking for is not in this basket")]
    AssetNotInBasket,

    #[error("The user's deposit amount exceeds the reserve limit of one or more of the assets")]
    DepositLimitExceeded,
    
    #[error("When pricing an asset, found a negative price")]
    NegativePrice,

    #[error("Failed to query token decimals")]
    FailedToQueryTokenDecimals,

    #[error("Failed to query token supply")]
    FailedToQueryTokenSupply,

    #[error("Price not found for asset")]
    PriceNotFound,

    #[error("Position is unhealthy")]
    PositionNotHealthy,

    #[error("Price to Uint128 received invalid price (expected_expo = {expected_expo:?}, expo = {expo:?})")]
    IncorrectDecimals {
        expo: i32,
        expected_expo: i32
    },
}

impl From<OverflowError> for ContractError {
    fn from(o: OverflowError) -> Self {
        StdError::from(o).into()
    }
}
