import { Program, Provider } from '@project-serum/anchor';
import { PublicKey } from '@solana/web3.js';
import { VoterStakeRegistry } from './types';

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
    const idl = await Program.fetchIdl(VSR_ID, provider);
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
