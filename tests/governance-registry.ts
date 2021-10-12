import * as anchor from '@project-serum/anchor';
import { Program } from '@project-serum/anchor';
import { PublicKey, Keypair, SystemProgram, SYSVAR_RENT_PUBKEY } from '@solana/web3.js';
import { TOKEN_PROGRAM_ID } from '@solana/spl-token';
import { GovernanceRegistry } from '../target/types/governance_registry';

describe('voting-rights', () => {
  anchor.setProvider(anchor.Provider.env());

  const program = anchor.workspace.GovernanceRegistry as Program<GovernanceRegistry>;

	// Initialized variables shared across tests.
	const realm = Keypair.generate().publicKey;
	const votingMintDecimals = 6;

	// Uninitialized variables shared across tests.
	let registrar: PublicKey, votingMint: PublicKey, voter: PublicKey;
	let registrarBump: number, votingMintBump: number, voterBump: number;

	it('Creates PDAs', async () => {
		const [_registrar, _registrarBump] = await PublicKey.findProgramAddress(
			[realm.toBuffer()],
			program.programId,
		);
		const [_votingMint, _votingMintBump] = await PublicKey.findProgramAddress(
			[_registrar.toBuffer()],
			program.programId,
		);
		const [_voter, _voterBump] = await PublicKey.findProgramAddress(
			[_registrar.toBuffer(), program.provider.wallet.publicKey.toBuffer()],
			program.programId,
		);

		registrar = _registrar;
		votingMint = _votingMint;
		voter = _voter;

		registrarBump = _registrarBump;
		votingMintBump = _votingMintBump;
		voterBump = _voterBump;
	});

  it('Initializes a registrar', async () => {
		await program.rpc.initRegistrar(registrarBump, votingMintBump, votingMintDecimals, {
			accounts: {
				registrar,
				votingMint,
				realm,
				authority: program.provider.wallet.publicKey,
				payer: program.provider.wallet.publicKey,
				systemProgram: SystemProgram.programId,
				tokenProgram: TOKEN_PROGRAM_ID,
				rent: SYSVAR_RENT_PUBKEY,
			}
		});

		const registrarAccount = await program.account.registrar.fetch(registrar);
		console.log(registrarAccount);
  });

	it('Initializes a voter', async () => {
		await program.rpc.initVoter(voterBump, {
			accounts: {
				voter,
				registrar,
				authority: program.provider.wallet.publicKey,
				systemProgram: SystemProgram.programId,
			}
		});

		const voterAccount = await program.account.voter.fetch(voter);
		console.log(voterAccount);
	});

	it('Adds an exchange rate', async () => {

	});

	it('Mints voting rights', async () => {

	});
});
