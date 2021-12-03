# Description

Voter-stake-registry is a voter weight addin for Solana's
[spl-governance program](https://github.com/solana-labs/solana-program-library/tree/master/governance).

With the addin enabled, the governance realm authority can:

- Control which token mints can be used to vote, and at what scaling factor.

  That means that tokens from mints other than the governing mint can be used
  to vote and that their relative weight can be set.

- Claw back locked tokens from user deposits where the user has enabled it.

  This is intended for use with token grants. Users would not enable clawback
  for normal deposits.

Users can:

- Deposit and withdraw tokens of the chosen mints to gain voting weight.

  When an addin is enabled, the default deposit/withdraw flow of the governing
  token mints is disabled in spl-governance. The addin adds back the ability
  to deposit and withdraw without lockup.

- Lock up tokens with different vesting schedules.

  The tokens will only be withdrawable once vested or the lock up has expired.
  Locked up tokens may have extra voting weight.

- Use their voting weight to vote on spl-governance proposals.


# Usage Scenarios

## Setup

To start using the addin, make a governance proposal with the spl-governance
realm authority to:
1. Deploy an instance of the voter-stake-registry.
2. Create a registrar for the realm with the `CreateRegistrar` instruction.
3. Add voting token mints to the registrar by calling the `CreateExchangeRate`
   instruction as often as desired.
4. Call the `SetRealmConfig` instruction on spl-governance to set the
   voter-weight-addin program id and thereby enable the addin.

## Deposit and Vote Without Lockup

1. Call `CreateVoter` on the addin (first time only). Use the same
   voter_authority that was used for registering with spl-governance.
2. Call `CreateDepositEntry` for the voter with `LockupKind::None`
   and the token mint for that tokens are to be deposited. (first time only)

   This creates a new deposit entry that can be used for depositing and
   withdrawing funds without lockup.
3. Call `Deposit` for the voter and same deposit entry id to deposit funds.
4. To vote, call `UpdateVoterWeightRecord` on the addin and then call `CastVote`
   on spl-governance in the same transaction, passing the voter weight record
   to both.
5. Withdraw funds with `Withdraw` once proposals have resolved.

## Give Grants of Locked Tokens

1. Ask the recepient to `CreateVoter` and `CreateDepositEntry` with the desired
   lock up period, vesting and `allow_clawback=true`. Double check the address
   and deposit entry id they communicate.
2. Make a proposal to call `Deposit` for depositing tokens into their locked
   deposit entry.
3. If necessary, later make a proposal to call `Clawback` on their deposit to
   retrieve all remaining locked tokens.


# License

This code is currently not free to use while in development.


# References:
* [spl-governance](https://github.com/solana-labs/solana-program-library/tree/master/governance)
