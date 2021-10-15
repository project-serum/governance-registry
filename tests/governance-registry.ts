import * as assert from 'assert';
import * as anchor from '@project-serum/anchor';
import { Program } from '@project-serum/anchor';
import { createMintAndVault } from '@project-serum/common';
import BN from 'bn.js';
import { PublicKey, Keypair, SystemProgram, SYSVAR_RENT_PUBKEY } from '@solana/web3.js';
import { Token, TOKEN_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID } from '@solana/spl-token';
import { GovernanceRegistry } from '../target/types/governance_registry';

describe('voting-rights', () => {
  anchor.setProvider(anchor.Provider.env());

  const program = anchor.workspace.GovernanceRegistry as Program<GovernanceRegistry>;

  // Initialized variables shared across tests.
  const realm = Keypair.generate().publicKey;
  const votingMintDecimals = 6;
  const tokenProgram = TOKEN_PROGRAM_ID;
  const associatedTokenProgram = ASSOCIATED_TOKEN_PROGRAM_ID;
  const rent = SYSVAR_RENT_PUBKEY;
  const systemProgram = SystemProgram.programId;

  // Uninitialized variables shared across tests.
  let registrar: PublicKey,
  votingMint: PublicKey,
  voter: PublicKey,
  votingToken: PublicKey,
  exchangeVaultA: PublicKey,
  exchangeVaultB: PublicKey;
  let registrarBump: number, votingMintBump: number, voterBump: number;
  let mintA: PublicKey, mintB: PublicKey, godA: PublicKey, godB: PublicKey;

  it('Creates tokens and mints', async () => {
    const decimals = 6;
    const [_mintA, _godA] = await createMintAndVault(
      program.provider,
      new BN("1000000000000000000"),
      undefined,
      decimals
    );
    const [_mintB, _godB] = await createMintAndVault(
      program.provider,
      new BN("1000000000000000000"),
      undefined,
      decimals
    );

    mintA = _mintA;
    mintB = _mintB;
    godA = _godA;
    godB = _godB;
  });

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
    votingToken = await Token.getAssociatedTokenAddress(
      associatedTokenProgram,
      tokenProgram,
      _votingMint,
      program.provider.wallet.publicKey,
    );
    exchangeVaultA = await Token.getAssociatedTokenAddress(
      associatedTokenProgram,
      tokenProgram,
      mintA,
      _registrar,
      true,
    );
    exchangeVaultB = await Token.getAssociatedTokenAddress(
      associatedTokenProgram,
      tokenProgram,
      mintB,
      _registrar,
      true,
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
        systemProgram,
        tokenProgram,
        rent,
      },
    });
  });

  it('Initializes a voter', async () => {
    await program.rpc.initVoter(voterBump, {
      accounts: {
        voter,
        votingToken,
        votingMint,
        registrar,
        authority: program.provider.wallet.publicKey,
        systemProgram,
        associatedTokenProgram,
        tokenProgram,
        rent,
      }
    });
  });

  it('Adds an exchange rate', async () => {
    const er = {
      isUsed: false,
      mint: mintA,
      rate: new BN(1),
    }
    await program.rpc.addExchangeRate(er, {
      accounts: {
        exchangeVault: exchangeVaultA,
        depositMint: mintA,
        registrar,
        authority: program.provider.wallet.publicKey,
        payer: program.provider.wallet.publicKey,
        rent,
        tokenProgram,
        associatedTokenProgram,
        systemProgram,
      }
    })
  });

  it('Deposits unlocked A tokens', async () => {
    const amount = new BN(10);
    await program.rpc.deposit(amount, {
      accounts: {
        voter,
        exchangeVault: exchangeVaultA,
        depositToken: godA,
        votingToken,
        authority: program.provider.wallet.publicKey,
        registrar,
        depositMint: mintA,
        votingMint,
        tokenProgram,
      },
    });
    const voterAccount = await program.account.voter.fetch(voter);
    console.log(voterAccount);


  });

  it ('Deposits locked tokens', async () => {

  });

  it('Mints voting rights', async () => {

  });
});
