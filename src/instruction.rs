use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    pubkey::Pubkey,
    instruction::{Instruction, AccountMeta},
    sysvar,
    entrypoint::ProgramResult,
};
use crate::state::{MAX_ADDRESS_SIZE, MAX_NETWORKS_SIZE};
use crate::error::BridgeError;
use mpl_token_metadata::state::DataV2;
use crate::util;

#[repr(C)]
#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct InitializeAdminArgs {
    pub seeds: [u8; 32],
}

#[repr(C)]
#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct TransferOwnershipArgs {
    pub new_admin: Pubkey,
    pub seeds: [u8; 32],
}

#[repr(C)]
#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct DepositArgs {
    pub network_to: String,
    pub receiver_address: String,
    // original collection address
    pub address: Option<String>,
    // original token id
    pub token_id: Option<String>,
    pub seeds: [u8; 32],
    pub nonce: [u8; 32],
}

impl DepositArgs {
    pub fn validate(&self) -> ProgramResult {
        if self.receiver_address.as_bytes().len() > MAX_ADDRESS_SIZE || self.network_to.as_bytes().len() > MAX_NETWORKS_SIZE {
            return Err(BridgeError::WrongArgsSize.into());
        }

        util::validate_option_str(&self.token_id, MAX_ADDRESS_SIZE)?;
        util::validate_option_str(&self.address, MAX_ADDRESS_SIZE)?;
        Ok(())
    }
}

#[repr(C)]
#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct WithdrawArgs {
    pub deposit_tx: String,
    pub network_from: String,
    pub sender_address: String,
    pub token_id: Option<String>,
    pub seeds: [u8; 32],
}

impl WithdrawArgs {
    pub fn validate(&self) -> ProgramResult {
        if self.sender_address.as_bytes().len() > MAX_ADDRESS_SIZE || self.network_from.as_bytes().len() > MAX_NETWORKS_SIZE {
            return Err(BridgeError::WrongArgsSize.into());
        }

        util::validate_option_str(&self.token_id, MAX_ADDRESS_SIZE)?;
        Ok(())
    }
}

#[repr(C)]
#[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug, Clone)]
pub struct MintArgs {
    pub data: DataV2,
    pub seeds: [u8; 32],
    pub verify: bool,
    pub token_id: Option<String>,
    pub address: Option<String>,
}

impl MintArgs {
    pub fn validate(&self) -> ProgramResult {
        util::validate_option_str(&self.token_id, MAX_ADDRESS_SIZE)?;
        util::validate_option_str(&self.address, MAX_ADDRESS_SIZE)?;
        Ok(())
    }
}


#[derive(BorshSerialize, BorshDeserialize, Clone)]
pub enum BridgeInstruction {
    /// Initialize new BridgeAdmin that will manage contract operations.
    ///
    /// Admin is fee payer.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` The BridgeAdmin account to initialize
    ///   1. `[writable,signer]` The admin account
    ///   2. `[]` System program
    ///   3. `[]` Rent sysvar
    InitializeAdmin(InitializeAdminArgs),

    /// Change admin in BridgeAdmin.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` The BridgeAdmin account
    ///   1. `[signer]` Current admin account
    ///
    TransferOwnership(TransferOwnershipArgs),

    /// Make NFT deposit on bridge.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[]` The BridgeAdmin account
    ///   1. `[]` The token mint account
    ///   2. `[writable]` The owner token associated account
    ///   3. `[writable]` The bridge token account
    ///   4. `[writable]` The new Deposit account
    ///   5. `[writable,signer]` The token owner account
    ///   6. `[]` Token program id
    ///   7. `[]` System program
    ///   8. `[]` Rent sysvar
    ///   9. `[]` Associated token program
    DepositMetaplex(DepositArgs),

    /// Make NFT withdraw from bridge.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[]` The BridgeAdmin account
    ///   1. `[]` The token mint account
    ///   2. `[writable,signer]` The owner account
    ///   3. `[writable]` The owner token associated account
    ///   4. `[writable]` The bridge token account
    ///   5. `[writable]` The new Withdraw account
    ///   6. `[signer]` The admin account
    ///   7. `[]` Token program id
    ///   8. `[]` System program
    ///   9. `[]` Rent sysvar
    ///   10. `[]` Associated token program
    WithdrawMetaplex(WithdrawArgs),

    /// Make NFT authored by bridge.
    /// Requires collection authored by bridge admin account.
    /// Mint account should be created before in same transaction.
    /// Also call verify collection on Metaplex program if verify=true was passed in arguments.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` The BridgeAdmin account
    ///
    /// Token mint account should be signed, but the signature can be derived from address and tokenId,
    /// if you sent it in instruction arguments.
    ///   1. `[writable,signed]` The token mint account
    ///   2. `[writable]` The bridge token account
    ///   3. `[writable]` The new metadata account
    ///   4. `[writable]` The new master edition account

    ///   5. `[signer]` The admin account
    ///   6. `[writable,signer]` The payer account
    ///
    ///   7. `[]` Token program id
    ///   8. `[]` Token metadata program id
    ///   9. `[]` Rent sysvar
    ///   10. `[]` System program
    ///   11. `[]` Associated token program
    ///
    /// Optional accounts (if verify=true)
    ///   12. `[]` The collection account
    ///   13. `[]` The collection metadata account
    ///   14. `[]` The collection master edition account
    MintMetaplex(MintArgs),
}

pub fn initialize_admin(
    program_id: Pubkey,
    bridge_admin: Pubkey,
    admin: Pubkey,
    seeds: [u8; 32],
) -> Instruction {
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(bridge_admin, false),
            AccountMeta::new(admin, true),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
        ],
        data: BridgeInstruction::InitializeAdmin(InitializeAdminArgs {
            seeds,
        }).try_to_vec().unwrap(),
    }
}

pub fn transfer_ownership(
    program_id: Pubkey,
    bridge_admin: Pubkey,
    admin: Pubkey,
    new_admin: Pubkey,
    seeds: [u8; 32],
) -> Instruction {
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(bridge_admin, false),
            AccountMeta::new_readonly(admin, true),
        ],
        data: BridgeInstruction::TransferOwnership(TransferOwnershipArgs {
            new_admin,
            seeds,
        }).try_to_vec().unwrap(),
    }
}

pub fn deposit_metaplex(
    program_id: Pubkey,
    bridge_admin: Pubkey,
    mint: Pubkey,
    owner_associated: Pubkey,
    bridge_associated: Pubkey,
    deposit: Pubkey,
    owner: Pubkey,
    seeds: [u8; 32],
    network_to: String,
    receiver_address: String,
    token_id: Option<String>,
    address: Option<String>,
    nonce: [u8; 32],
) -> Instruction {
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(bridge_admin, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(owner_associated, false),
            AccountMeta::new(bridge_associated, false),
            AccountMeta::new(deposit, false),
            AccountMeta::new(owner, true),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
        ],
        data: BridgeInstruction::DepositMetaplex(DepositArgs {
            network_to,
            receiver_address,
            seeds,
            token_id,
            address,
            nonce,
        }).try_to_vec().unwrap(),
    }
}

pub fn withdraw_metaplex(
    program_id: Pubkey,
    bridge_admin: Pubkey,
    mint: Pubkey,
    owner: Pubkey,
    owner_associated: Pubkey,
    bridge_associated: Pubkey,
    withdraw: Pubkey,
    admin: Pubkey,
    seeds: [u8; 32],
    deposit_tx: String,
    network_from: String,
    sender_address: String,
    token_id: Option<String>,
) -> Instruction {
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(bridge_admin, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(owner, true),
            AccountMeta::new(owner_associated, false),
            AccountMeta::new(bridge_associated, false),
            AccountMeta::new(withdraw, false),
            AccountMeta::new_readonly(admin, true),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
        ],
        data: BridgeInstruction::WithdrawMetaplex(WithdrawArgs {
            deposit_tx,
            network_from,
            seeds,
            token_id,
            sender_address,
        }).try_to_vec().unwrap(),
    }
}