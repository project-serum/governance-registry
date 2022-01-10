import { Provider, Wallet } from '@project-serum/anchor';
import { Connection, Keypair } from '@solana/web3.js';
import { VsrClient } from '../src';

async function main() {
  const options = Provider.defaultOptions();
  const connection = new Connection('https://api.devnet.solana.com', options);
  const wallet = new Wallet(Keypair.generate());
  const provider = new Provider(connection, wallet, options);
  const client = await VsrClient.connect(provider, true);
}

main();
