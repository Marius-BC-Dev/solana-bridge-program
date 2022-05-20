use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvar::{rent::Rent, Sysvar},
};
use mpl_token_metadata::state::Data;
use borsh::{BorshDeserialize, BorshSerialize};
use crate::{
    instruction::BridgeInstruction,
    state::{BridgeAdmin, BRIDGE_ADMIN_SIZE},
    error::BridgeError,
};

pub fn process_instruction<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    input: &[u8],
) -> ProgramResult {
    let instruction = BridgeInstruction::try_from_slice(input)?;
    match instruction {
        BridgeInstruction::InitializeAdmin(args) => {
            msg!("Instruction: Create Bridge Admin");
            process_init_admin(program_id, accounts, args.admin)
        }
        BridgeInstruction::TransferOwnership(args) => {
            msg!("Instruction: Transfer Bridge Admin ownership");
            process_transfer_ownership(program_id, accounts, args.new_admin)
        }
        BridgeInstruction::DepositMetaplex(args) => {
            msg!("Instruction: Deposit token");
            process_deposit_metaplex(program_id, accounts, args.network_to, args.receiver_address, args.nonce)
        }
        BridgeInstruction::WithdrawMetaplex(args) => {
            msg!("Instruction: Withdraw token");
            process_withdraw_metaplex(program_id, accounts, args.deposit_tx, args.network_from, args.sender_address, args.data)
        }
    }
}

pub fn process_init_admin<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    admin: Pubkey,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let bridge_admin_account_info = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;

    let mut bridge_admin: BridgeAdmin = BorshDeserialize::deserialize(&mut bridge_admin_account_info.data.borrow_mut().as_ref())?;
    if bridge_admin.is_initialized {
        return Err(BridgeError::AlreadyInUse.into());
    }

    if !bridge_admin_account_info.data_len() != BRIDGE_ADMIN_SIZE {
        return Err(BridgeError::WrongDataLen.into());
    }

    let rent = Rent::from_account_info(rent_info)?;
    if !rent.is_exempt(bridge_admin_account_info.lamports(), BRIDGE_ADMIN_SIZE) {
        return Err(BridgeError::NotRentExempt.into());
    }

    bridge_admin.admin = admin;
    bridge_admin.serialize(&mut *bridge_admin_account_info.data.borrow_mut())?;
    Ok(())
}

pub fn process_transfer_ownership<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    new_admin: Pubkey,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let bridge_admin_account_info = next_account_info(account_info_iter)?;
    let current_admin_account_info = next_account_info(account_info_iter)?;

    let mut bridge_admin: BridgeAdmin = BorshDeserialize::deserialize(&mut bridge_admin_account_info.data.borrow_mut().as_ref())?;
    if !bridge_admin.is_initialized {
        return Err(BridgeError::NotInitialized.into());
    }

    if !current_admin_account_info.is_signer {
        return Err(BridgeError::UnsignedAdmin.into());
    }

    bridge_admin.admin = new_admin;

    bridge_admin.serialize(&mut *bridge_admin_account_info.data.borrow_mut())?;
    Ok(())
}

pub fn process_deposit_metaplex<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    network: String,
    receiver: String,
    nonce: String,
) -> ProgramResult {
    // TODO
    Ok(())
}

pub fn process_withdraw_metaplex<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    tx: String,
    network: String,
    sender: String,
    data: Data,
) -> ProgramResult {
    // TODO
    Ok(())
}