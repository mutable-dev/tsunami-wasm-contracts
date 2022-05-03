const fetch = require('isomorphic-fetch');
const { MsgExecuteContract, MnemonicKey, Coins, LCDClient } = require('@terra-money/terra.js');

async function main(offer_asset, offer_amount, ask_asset) {
    const contract = process.env.CONTRACT;
    if (!contract) {
        console.log("Please set CONTRACT environment variable to the contract address");
        exit(1);
    }

    const lpContract = process.env.LP_CONTRACT;
    if (!lpContract) {
        console.log("Please set LP_CONTRACT environment variable to the contract address");
        exit(1);
    }

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
    console.log("before swap, you have:");
    console.log(balanceBefore.toData());

    const msg = new MsgExecuteContract(
        wallet.key.accAddress,
        contract,
        {
          swap: {
            ask_asset: {
              native_token: {
                denom: ask_asset
              }
            },
            offer_asset: {
              amount: offer_amount,
              info: {
                native_token: { 
                  denom: offer_asset
                }
              },
            },
            sender: wallet.key.accAddress,
          }
        },
        {[offer_asset]: offer_amount},
      );

    const tx = await wallet.createAndSignTx({msgs: [msg]});
    const result = await lcd.tx.broadcast(tx);
    console.log(result);


    const [balanceAfter] = await lcd.bank.balance(mk.accAddress);
    console.log("after swap, you have:");
    console.log(balanceAfter.toData());
}

const args = process.argv.slice(2);
if (args.length < 3) {
    console.log("Usage: node swap.js <offer_denom> <offer_amount> <ask_denom>");
    exit(1);
}

main(args[0], args[1], args[2]);
