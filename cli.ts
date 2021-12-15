import * as anchor from "@project-serum/anchor";

async function main() {
  const programId = new anchor.web3.PublicKey(
    "4Q6WW2ouZ6V3iaNm56MTd5n2tnTm4C5fiH8miFHnAFHo"
  );
  const clusterUrl = "https://api.devnet.solana.com";
  const throwAway = new anchor.web3.Keypair();

  const connection = new anchor.web3.Connection(clusterUrl);

  const walletWrapper = new anchor.Wallet(throwAway);

  const provider = new anchor.Provider(connection, walletWrapper, {
    preflightCommitment: "processed",
  });

  const idl = await anchor.Program.fetchIdl(programId, provider);

  const program = new anchor.Program(idl, programId, provider);

  console.log("program id from anchor", program.programId.toBase58());

  return program;
}

main();
