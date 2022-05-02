const fetch = require('isomorphic-fetch');
const { Coins, LCDClient } = require('@terra-money/terra.js');

async function main() {
    const contract = process.env.CONTRACT;
    if (!contract) {
        console.log("Please set CONTRACT environment variable to the contract address");
        exit(1);
    }

    console.log(`contract: ${contract}`);

    const gasPrices = await (await fetch('https://bombay-fcd.terra.dev/v1/txs/gas_prices')).json();
    const gasPricesCoins = new Coins(gasPrices);

    const lcd = new LCDClient({
        URL: "https://bombay-lcd.terra.dev/",
        chainID: "bombay-12",
        gasPrices: gasPricesCoins,
        gasAdjustment: "1.5",
        gas: 10000000,
    });
    const result = await lcd.wasm.contractQuery(
      contract, 
      {
        "basket": {}
      }
    );

    console.log(result);
}

main();
