




#[test]
fn test_pyth_usd() {

    use pyth_sdk_terra::*;
    const USD_VALUE_PRECISION: i32 = -9;

    // Define asset
    let token_amount = 123_456_789;
    let token_decimals = 3;

    // Mock PriceFeed
    let price: i64 = 10_i64.pow(10);
    let price_feed: PriceFeed = PriceFeed::new(
        PriceIdentifier::new([0; 32]),
        PriceStatus::Trading,
        0,
        USD_VALUE_PRECISION,
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
    );
    let pyth_price: Price = price_feed.get_current_price().unwrap();

    // Use price to compute usd_value of asset
    let usd_value: Price = Price::price_basket(
        &[(pyth_price, token_amount, -token_decimals)], USD_VALUE_PRECISION).unwrap();

    println!("The USD value of {} x 10^{} tokens is {} x 10^{} USD, since the price is {:} x 10^{} USD per token",
        token_amount,
        -token_decimals,
        usd_value.price,
        usd_value.expo,
        pyth_price.price,
        pyth_price.expo,
    );

}

#[test]
fn test_invert_price() {

    use pyth_sdk_terra::*;
    const USD_VALUE_PRECISION: i32 = -3;


    // Mock PriceFeed
    let price: i64 = 10_i64.pow(-USD_VALUE_PRECISION as u32 + 1); // $10/token
    let price_feed: PriceFeed = PriceFeed::new(
        PriceIdentifier::new([0; 32]),
        PriceStatus::Trading,
        0,
        USD_VALUE_PRECISION,
        5,
        10_000_000,
        PriceIdentifier::new([0; 32]),
        price,
        (price/10) as u64,
        0,
        0,
        0,
        0,
        0
    );
    let pyth_price: Price = price_feed.get_current_price().unwrap();

    println!("The pyth_price for the tokens is ({} +/- {}) x 10^{} token/USD", pyth_price.price, pyth_price.conf, pyth_price.expo);

    let unit_price: PriceFeed = PriceFeed::new(
        PriceIdentifier::new([0; 32]),
        PriceStatus::Trading,
        0,
        USD_VALUE_PRECISION,
        5,
        10_000_000,
        PriceIdentifier::new([0; 32]),
        10_i64.pow(-USD_VALUE_PRECISION as u32),
        0,
        0,
        0,
        0,
        0,
        0
    );
    let inverted_price: Price = unit_price.get_current_price().unwrap().div(&pyth_price).unwrap();

    println!("The inverted pyth_price for the tokens is ({} +/- {}) x 10^{} token/USD", inverted_price.price, inverted_price.conf, inverted_price.expo);

}

