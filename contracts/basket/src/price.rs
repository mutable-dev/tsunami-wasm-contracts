use cosmwasm_std::Uint128;
use crate::error::ContractError;

#[derive(Copy, Clone, Debug)]
pub struct PythPrice {
    pub pyth_price: pyth_sdk_terra::Price,
}

impl PythPrice {
    pub fn new(pyth_price: pyth_sdk_terra::Price) -> Self {
        PythPrice { pyth_price }
    }

    // TODO: should pass in an enum that is either offer, ask, USD, and check the expo of the price going in
    #[allow(non_snake_case)]
    pub fn to_Uint128(&self, expected_expo: i32) -> Result<Uint128, ContractError> {
        // Check for positive price
        if self.pyth_price.price < 0 { return Err(ContractError::NegativePrice) }

        // Check for expected expo
        if self.pyth_price.expo != expected_expo { return Err(ContractError::IncorrectDecimals { expo: self.pyth_price.expo, expected_expo }) }
    
        Ok(Uint128::new(self.pyth_price.price as u128))
    }
}
