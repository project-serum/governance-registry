import * as assert from "assert";
import * as anchor from "@project-serum/anchor";
import { Program } from "@project-serum/anchor";
import { createMintAndVault } from "@project-serum/common";
import BN from "bn.js";
import {
  PublicKey,
  Keypair,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
} from "@solana/web3.js";
import {
  Token,
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { GovernanceRegistry } from "../target/types/governance_registry";

describe("voting-rights", () => {
  anchor.setProvider(anchor.Provider.env());

  const program = anchor.workspace
    .GovernanceRegistry as Program<GovernanceRegistry>;

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
  let tokenAClient: Token, tokenBClient: Token, votingTokenClient: Token;

  it("Creates tokens and mints", async () => {
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

  it("Creates PDAs", async () => {
    const [_registrar, _registrarBump] = await PublicKey.findProgramAddress(
      [realm.toBuffer()],
      program.programId
    );
    const [_votingMint, _votingMintBump] = await PublicKey.findProgramAddress(
      [_registrar.toBuffer()],
      program.programId
    );
    const [_voter, _voterBump] = await PublicKey.findProgramAddress(
      [_registrar.toBuffer(), program.provider.wallet.publicKey.toBuffer()],
      program.programId
    );
    votingToken = await Token.getAssociatedTokenAddress(
      associatedTokenProgram,
      tokenProgram,
      _votingMint,
      program.provider.wallet.publicKey
    );
    exchangeVaultA = await Token.getAssociatedTokenAddress(
      associatedTokenProgram,
      tokenProgram,
      mintA,
      _registrar,
      true
    );
    exchangeVaultB = await Token.getAssociatedTokenAddress(
      associatedTokenProgram,
      tokenProgram,
      mintB,
      _registrar,
      true
    );

    registrar = _registrar;
    votingMint = _votingMint;
    voter = _voter;

    registrarBump = _registrarBump;
    votingMintBump = _votingMintBump;
    voterBump = _voterBump;
  });

  it("Creates token clients", async () => {
    tokenAClient = new Token(
      program.provider.connection,
      mintA,
      TOKEN_PROGRAM_ID,
      // @ts-ignore
      program.provider.wallet.payer
    );
    tokenBClient = new Token(
      program.provider.connection,
      mintB,
      TOKEN_PROGRAM_ID,
      // @ts-ignore
      program.provider.wallet.payer
    );
    votingTokenClient = new Token(
      program.provider.connection,
      votingMint,
      TOKEN_PROGRAM_ID,
      // @ts-ignore
      program.provider.wallet.payer
    );
  });

  it("Initializes a registrar", async () => {
    await program.rpc.createRegistrar(
			new BN(0),
      registrarBump,
      votingMintBump,
      votingMintDecimals,
      {
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
      }
    );
  });

  it("Initializes a voter", async () => {
    await program.rpc.createVoter(voterBump, {
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
      },
    });
  });

  it("Adds an exchange rate", async () => {
    const er = {
      isUsed: false,
      mint: mintA,
      rate: new BN(1),
    };
    await program.rpc.createExchangeRate(er, {
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
      },
    });
  });

  it("Deposits cliff locked A tokens", async () => {
    const amount = new BN(10);
		const kind = { cliff: {} };
		const days = 1;
    await program.rpc.createDeposit(kind, amount, days, {
      accounts: {
        deposit: {
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
      },
    });

    const voterAccount = await program.account.voter.fetch(voter);
    const deposit = voterAccount.deposits[0];
    assert.ok(deposit.isUsed);
    assert.ok(deposit.amountDeposited.toNumber() === 10);
    assert.ok(deposit.rateIdx === 0);

    const vtAccount = await votingTokenClient.getAccountInfo(votingToken);
    assert.ok(vtAccount.amount.toNumber() === 10);
  });

	/*
  it("Withdraws cliff locked A tokens", async () => {
    const depositId = 0;
    const amount = new BN(10);
    await program.rpc.withdraw(depositId, amount, {
      accounts: {
        registrar,
        voter,
        exchangeVault: exchangeVaultA,
        withdrawMint: mintA,
        votingToken,
        votingMint,
        destination: godA,
        authority: program.provider.wallet.publicKey,
        tokenProgram,
      },
    });

    const voterAccount = await program.account.voter.fetch(voter);
    const deposit = voterAccount.deposits[0];
    assert.ok(deposit.isUsed);
    assert.ok(deposit.amount.toNumber() === 0);
    assert.ok(deposit.rateIdx === 0);

    const vtAccount = await votingTokenClient.getAccountInfo(votingToken);
    assert.ok(vtAccount.amount.toNumber() === 0);
  });
	*/

  it("Deposits daily locked A tokens", async () => {
    const amount = new BN(10);
		const kind = { daily: {} };
		const days = 1;
    await program.rpc.createDeposit(kind, amount, days, {
      accounts: {
        deposit: {
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
      },
    });

    const voterAccount = await program.account.voter.fetch(voter);
    const deposit = voterAccount.deposits[0];
    assert.ok(deposit.isUsed);
    assert.ok(deposit.amountDeposited.toNumber() === 10);
    assert.ok(deposit.rateIdx === 0);
  });
});
