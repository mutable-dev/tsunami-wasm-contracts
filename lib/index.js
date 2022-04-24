module.exports = ({ wallets, refs, config, client }) => ({
  getCount: () => client.query("basket", { get_count: {} }),
  increment: (signer = wallets.validator) =>
    client.execute(signer, "basket", { increment: {} }),
});
