const fetch = require('isomorphic-fetch');
const { MsgExecuteContract, MnemonicKey, Coins, LCDClient } = require('@terra-money/terra.js');

async function main() {
    // Fetch gas prices and convert to `Coin` format.
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

    const wallet = lcd.wallet(mk);
    const [balanceBefore] = await lcd.bank.balance(mk.accAddress);
    console.log(balanceBefore.toData());

    const contract = "terra122dgdy3a6mwlru6deqynrsm3n7e0qax999q3za";

    const msg = new MsgExecuteContract(
        wallet.key.accAddress,
        contract,
        {
          "swap": {
            "ask_asset": {
              "native_token": {
                "denom": "uusd"
              }
            },
            "offer_asset": {
              "amount": "100000",
              "info": {
                "native_token": { 
                  "denom": "uluna"
                }
              },
            },
            "sender": wallet.key.accAddress,
          }
        },
        {"uluna": "100000"},
      );

    const tx = wallet.createAndSignTx({msgs: [msg]});
    // const result = lcd.tx.broadcast(tx);
    // console.log(result);


    // [balanceAfter] = await lcd.bank.balance(mk.accAddress);
    // console.log(balanceAfter.toData());
}

main();
