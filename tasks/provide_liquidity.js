const fetch = require('isomorphic-fetch');
const { MsgExecuteContract, MnemonicKey, Coins, LCDClient } = require('@terra-money/terra.js');

async function main(denom, amount) {
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

    console.log(`contract: ${contract}, lpContract: ${lpContract}`);

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
    const [balanceBefore] = await lcd.bank.balance(mk.accAddress);

    const lpBalanceBefore = await lcd.wasm.contractQuery(
        lpContract, 
        {
            balance: { address: mk.accAddress }
        },
    );

    console.log("before provide liquidity, you have:");
    console.log(balanceBefore.toData());
    console.log("LP token:");
    console.log(lpBalanceBefore);

    const wallet = lcd.wallet(mk);

    const msg = new MsgExecuteContract(
        wallet.key.accAddress,
        contract,
        {
            deposit_liquidity: {
                assets: [{
                    amount,
                    info: {
                        native_token: {
                            denom,
                        }
                    },
                }]
            }
        },
        {[denom]: amount},
    );

    const tx = await wallet.createAndSignTx({msgs: [msg]});
    const result = await lcd.tx.broadcast(tx);
    console.log(`provide liquidity tx result: ${result}`);

    const [balanceAfter] = await lcd.bank.balance(mk.accAddress);
    const lpBalanceAfter = await lcd.wasm.contractQuery(
        lpContract, 
        {
            balance: { address: mk.accAddress }
        },
    );

    console.log("after provide liquidity, you have:");
    console.log(balanceAfter.toData());
    console.log("LP token:");
    console.log(lpBalanceAfter);
}

const args = process.argv.slice(2);
if (args.length < 2) {
    console.log("Usage: node provide_liquidity.js <denom> <amount>");
    exit(1);
}

main(args[0], args[1]);
