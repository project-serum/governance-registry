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
    votingMintA: PublicKey,
    votingMintB: PublicKey,
    voter: PublicKey,
    voterWeightRecord: PublicKey,
    votingToken: PublicKey,
    exchangeVaultA: PublicKey,
    exchangeVaultB: PublicKey;
  let registrarBump: number,
    votingMintBumpA: number,
    votingMintBumpB: number,
    voterBump: number,
    voterWeightRecordBump: number;
  let mintA: PublicKey,
    mintB: PublicKey,
    godA: PublicKey,
    godB: PublicKey,
    realmCommunityMint: PublicKey;
  let tokenAClient: Token,
    tokenBClient: Token,
    votingTokenClientA: Token,
    votingTokenClientB: Token;

  it("Creates tokens and mints", async () => {
    const [_mintA, _godA] = await createMintAndVault(
      program.provider,
      new BN("1000000000000000000"),
      undefined,
      6
    );
    const [_mintB, _godB] = await createMintAndVault(
      program.provider,
      new BN("1000000000000000000"),
      undefined,
      0
    );

    mintA = _mintA;
    mintB = _mintB;
    godA = _godA;
    godB = _godB;
    realmCommunityMint = mintA;
  });

  it("Creates PDAs", async () => {
    const [_registrar, _registrarBump] = await PublicKey.findProgramAddress(
      [realm.toBuffer()],
      program.programId
    );
    const [_votingMintA, _votingMintBumpA] = await PublicKey.findProgramAddress(
      [_registrar.toBuffer(), mintA.toBuffer()],
      program.programId
    );
    const [_votingMintB, _votingMintBumpB] = await PublicKey.findProgramAddress(
      [_registrar.toBuffer(), mintB.toBuffer()],
      program.programId
    );
    const [_voter, _voterBump] = await PublicKey.findProgramAddress(
      [_registrar.toBuffer(), program.provider.wallet.publicKey.toBuffer()],
      program.programId
    );
    const [_voterWeightRecord, _voterWeightRecordBump] =
      await PublicKey.findProgramAddress(
        [
          anchor.utils.bytes.utf8.encode("voter-weight-record"),
          _registrar.toBuffer(),
          program.provider.wallet.publicKey.toBuffer(),
        ],
        program.programId
      );
    votingToken = await Token.getAssociatedTokenAddress(
      associatedTokenProgram,
      tokenProgram,
      _votingMintA,
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
    votingMintA = _votingMintA;
    votingMintB = _votingMintB;
    voter = _voter;

    registrarBump = _registrarBump;
    votingMintBumpA = _votingMintBumpA;
    votingMintBumpB = _votingMintBumpB;
    voterBump = _voterBump;
    voterWeightRecord = _voterWeightRecord;
    voterWeightRecordBump = _voterWeightRecordBump;
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
    votingTokenClientA = new Token(
      program.provider.connection,
      votingMintA,
      TOKEN_PROGRAM_ID,
      // @ts-ignore
      program.provider.wallet.payer
    );
    votingTokenClientB = new Token(
      program.provider.connection,
      votingMintB,
      TOKEN_PROGRAM_ID,
      // @ts-ignore
      program.provider.wallet.payer
    );
  });

  it("Initializes a registrar", async () => {
    await program.rpc.createRegistrar(new BN(0), 6, registrarBump, {
      accounts: {
        registrar,
        realm,
        realmCommunityMint,
        authority: program.provider.wallet.publicKey,
        payer: program.provider.wallet.publicKey,
        systemProgram,
        tokenProgram,
        rent,
      },
    });
  });

  it("Adds an exchange rate A", async () => {
    const er = {
      mint: mintA,
      rate: new BN(1),
      decimals: 6,
    };
    await program.rpc.createExchangeRate(0, er, {
      accounts: {
        exchangeVault: exchangeVaultA,
        depositMint: mintA,
        votingMint: votingMintA,
        registrar,
        authority: program.provider.wallet.publicKey,
        rent,
        tokenProgram,
        associatedTokenProgram,
        systemProgram,
      },
    });
  });

  it("Adds an exchange rate B", async () => {
    const er = {
      mint: mintB,
      rate: new BN(1000000),
      decimals: 0,
    };
    await program.rpc.createExchangeRate(1, er, {
      accounts: {
        exchangeVault: exchangeVaultB,
        depositMint: mintB,
        votingMint: votingMintB,
        registrar,
        authority: program.provider.wallet.publicKey,
        rent,
        tokenProgram,
        associatedTokenProgram,
        systemProgram,
      },
    });
  });

  it("Initializes a voter", async () => {
    await program.rpc.createVoter(voterBump, voterWeightRecordBump, {
      accounts: {
        voter,
        voterWeightRecord,
        registrar,
        authority: program.provider.wallet.publicKey,
        payer: program.provider.wallet.publicKey,
        systemProgram,
        associatedTokenProgram,
        tokenProgram,
        rent,
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
          votingMint: votingMintA,
          tokenProgram,
          systemProgram,
          associatedTokenProgram,
          rent,
        },
      },
    });

    const voterAccount = await program.account.voter.fetch(voter);
    const deposit = voterAccount.deposits[0];
    assert.ok(deposit.isUsed);
    assert.ok(deposit.amountDeposited.toNumber() === 10);
    assert.ok(deposit.rateIdx === 0);

    const vtAccount = await votingTokenClientA.getAccountInfo(votingToken);
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
          votingMint: votingMintA,
          tokenProgram,
          systemProgram,
          associatedTokenProgram,
          rent,
        },
      },
    });

    const voterAccount = await program.account.voter.fetch(voter);
    const deposit = voterAccount.deposits[0];
    assert.ok(deposit.isUsed);
    assert.ok(deposit.amountDeposited.toNumber() === 10);
    assert.ok(deposit.rateIdx === 0);
  });

  it("Updates a vote weight record", async () => {
    await program.rpc.updateVoterWeightRecord({
      accounts: {
        registrar,
        voter,
        voterWeightRecord,
        authority: program.provider.wallet.publicKey,
        systemProgram,
      },
    });
  });
});
