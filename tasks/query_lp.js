const fetch = require('isomorphic-fetch');
const { MnemonicKey, Coins, LCDClient } = require('@terra-money/terra.js');

async function main() {
    const lpContract = process.env.LP_CONTRACT;
    if (!lpContract) {
        console.log("Please set LP_CONTRACT environment variable to the contract address");
        exit(1);
    }

    console.log(`contract: ${lpContract}`);

    const gasPrices = await (await fetch('https://bombay-fcd.terra.dev/v1/txs/gas_prices')).json();
    const gasPricesCoins = new Coins(gasPrices);

    const lcd = new LCDClient({
        URL: "https://bombay-lcd.terra.dev/",
        chainID: "bombay-12",
        gasPrices: gasPricesCoins,
        gasAdjustment: "1.5",
        gas: 10000000,
    });

    const mk = new MnemonicKey({
        mnemonic: 'notice oak worry limit basic speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius',
    });

    const result = await lcd.wasm.contractQuery(
        lpContract, 
        {
            balance: { address: mk.accAddress }
        },
    );

    console.log(result);
}

main();
