import { Program, Provider } from '@project-serum/anchor';
import { PublicKey } from '@solana/web3.js';
import { VoterStakeRegistry } from './voter_stake_registry';
import IDL from './voter_stake_registry.json';

export const VSR_ID = new PublicKey(
  '4Q6WW2ouZ6V3iaNm56MTd5n2tnTm4C5fiH8miFHnAFHo',
);

export class VsrClient {
  constructor(
    public program: Program<VoterStakeRegistry>,
    public devnet?: boolean,
  ) {}

  static async connect(
    provider: Provider,
    devnet?: boolean,
  ): Promise<VsrClient> {
    // fixme: when we push idl to mainnet we could use this
    // const idl = await Program.fetchIdl(VSR_ID, provider);
    const idl = IDL;

    return new VsrClient(
      new Program<VoterStakeRegistry>(
        idl as VoterStakeRegistry,
        VSR_ID,
        provider,
      ),
      devnet,
    );
  }
}
