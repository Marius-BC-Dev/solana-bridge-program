use std::cmp::max;

use borsh::{
    BorshDeserialize, BorshSerialize,
};
use mpl_token_metadata::{
    instructions::{CreateMasterEditionV3, CreateMetadataAccountV3, VerifyCollection, CreateMetadataAccountV3InstructionArgs},
    types::{DataV2, TokenStandard},
    accounts::Metadata,
};
use solana_program::{
    account_info::{AccountInfo, next_account_info},
    entrypoint::ProgramResult, hash, msg,
    program::{invoke, invoke_signed}, pubkey::Pubkey, secp256k1_recover::{SECP256K1_PUBLIC_KEY_LENGTH, SECP256K1_SIGNATURE_LENGTH}, system_instruction,
    sysvar::{rent::Rent, Sysvar},
    system_program as g_system_program,
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token::{
    instruction::{initialize_mint, mint_to, transfer},
    solana_program::program_pack::Pack,
    state::Mint,
};
use spl_token::instruction::burn;

use crate::{
    state::BridgeAdmin,
    state::Withdraw,
};
use crate::merkle::{Data, TransferData, Content};
use solana_program::sysvar::instructions::{load_current_index_checked, load_instruction_at_checked};
use lib::merkle::{get_merkle_root};
use lib::ecdsa::verify_ecdsa_signature;
use lib::instructions::bridge::{BridgeInstruction, SignedMetadata};
use lib::instructions::InstructionValidation;
use lib::error::LibError;
use crate::state::{BRIDGE_ADMIN_SIZE, WITHDRAW_SIZE};

pub fn process_instruction<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    input: &[u8],
) -> ProgramResult {
    let instruction = BridgeInstruction::try_from_slice(input)?;
    match instruction {
        BridgeInstruction::InitializeAdmin(args) => {
            msg!("Instruction: Create Bridge Admin");
            process_init_admin(program_id, accounts, args.seeds, args.public_key, args.commission_program)
        }
        BridgeInstruction::TransferOwnership(args) => {
            msg!("Instruction: Transfer Bridge Admin ownership");
            process_transfer_ownership(program_id, accounts, args.seeds, args.new_public_key, args.signature, args.recovery_id)
        }
        BridgeInstruction::DepositNative(args) => {
            msg!("Instruction: Deposit SOL");
            args.validate()?;
            process_deposit_native(program_id, accounts, args.seeds, args.network_to, args.receiver_address, args.amount)
        }
        BridgeInstruction::DepositFT(args) => {
            msg!("Instruction: Deposit FT");
            args.validate()?;
            process_deposit_ft(program_id, accounts, args.seeds, args.network_to, args.receiver_address, args.amount, args.token_seed)
        }
        BridgeInstruction::DepositNFT(args) => {
            msg!("Instruction: Deposit NFT");
            args.validate()?;
            process_deposit_nft(program_id, accounts, args.seeds, args.network_to, args.receiver_address, args.token_seed)
        }

        BridgeInstruction::WithdrawNative(args) => {
            msg!("Instruction: Withdraw SOL");
            args.validate()?;
            process_withdraw_native(program_id, accounts, args.seeds, args.signature, args.recovery_id, args.path, args.origin, args.amount)
        }

        BridgeInstruction::WithdrawFT(args) => {
            msg!("Instruction: Withdraw FT");
            args.validate()?;
            process_withdraw_ft(program_id, accounts, args.seeds, args.signature, args.recovery_id, args.path, args.origin, args.amount, args.token_seed, args.signed_meta)
        }

        BridgeInstruction::WithdrawNFT(args) => {
            msg!("Instruction: Withdraw NFT");
            args.validate()?;
            process_withdraw_nft(program_id, accounts, args.seeds, args.signature, args.recovery_id, args.path, args.origin, args.token_seed, args.signed_meta)
        }

        BridgeInstruction::MintCollection(args) => {
            msg!("Instruction: Mint Collection");
            args.validate()?;
            process_create_collection(program_id, accounts, args.seeds, args.data, args.token_seed)
        }
    }
}

pub fn process_init_admin<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    seeds: [u8; 32],
    public_key: [u8; SECP256K1_PUBLIC_KEY_LENGTH],
    commission_program: Pubkey,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let bridge_admin_info = next_account_info(account_info_iter)?;
    let fee_payer_info = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;

    let bridge_key = Pubkey::create_program_address(&[&seeds], &program_id)?;
    if bridge_key != *bridge_admin_info.key {
        return Err(LibError::WrongSeeds.into());
    }

    lib::call_create_account(
        fee_payer_info,
        bridge_admin_info,
        rent_info,
        system_program,
        BRIDGE_ADMIN_SIZE,
        program_id,
        &[&seeds],
    )?;

    let mut bridge_admin: BridgeAdmin = BorshDeserialize::deserialize(&mut bridge_admin_info.data.borrow_mut().as_ref())?;
    if bridge_admin.is_initialized {
        return Err(LibError::AlreadyInUse.into());
    }

    bridge_admin.public_key = public_key;
    bridge_admin.is_initialized = true;
    bridge_admin.commission_program = commission_program;
    bridge_admin.serialize(&mut *bridge_admin_info.data.borrow_mut())?;
    Ok(())
}

pub fn process_transfer_ownership<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    seeds: [u8; 32],
    new_public_key: [u8; SECP256K1_PUBLIC_KEY_LENGTH],
    signature: [u8; SECP256K1_SIGNATURE_LENGTH],
    recovery_id: u8,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let bridge_admin_info = next_account_info(account_info_iter)?;

    let bridge_admin_key = Pubkey::create_program_address(&[&seeds], &program_id)?;
    if bridge_admin_key != *bridge_admin_info.key {
        return Err(LibError::WrongSeeds.into());
    }

    let mut bridge_admin: BridgeAdmin = BorshDeserialize::deserialize(&mut bridge_admin_info.data.borrow_mut().as_ref())?;
    if !bridge_admin.is_initialized {
        return Err(LibError::NotInitialized.into());
    }


    verify_ecdsa_signature(solana_program::keccak::hash(new_public_key.as_slice()).as_ref(), signature.as_slice(), recovery_id, bridge_admin.public_key)?;

    bridge_admin.public_key = new_public_key;
    bridge_admin.serialize(&mut *bridge_admin_info.data.borrow_mut())?;
    Ok(())
}


pub fn process_deposit_native<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    seeds: [u8; 32],
    network: String,
    receiver: String,
    amount: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let bridge_admin_info = next_account_info(account_info_iter)?;
    let owner_info = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;
    let sysvar_info = next_account_info(account_info_iter)?;

    let bridge_admin_key = Pubkey::create_program_address(&[&seeds], &program_id)?;
    if *bridge_admin_info.key != bridge_admin_key {
        return Err(LibError::WrongSeeds.into());
    }

    let bridge_admin: BridgeAdmin = BorshDeserialize::deserialize(&mut bridge_admin_info.data.borrow_mut().as_ref())?;
    if !bridge_admin.is_initialized {
        return Err(LibError::NotInitialized.into());
    }

    verify_commission_charged( bridge_admin_info, sysvar_info, &bridge_admin, lib::TokenType::Native, amount)?;

    let transfer_tokens_instruction = solana_program::system_instruction::transfer(
        owner_info.key,
        bridge_admin_info.key,
        amount,
    );

    msg!("Transferring token");
    invoke(
        &transfer_tokens_instruction,
        &[
            owner_info.clone(),
            bridge_admin_info.clone(),
        ],
    )?;

    Ok(())
}

pub fn process_deposit_ft<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    seeds: [u8; 32],
    network: String,
    receiver: String,
    amount: u64,
    token_seed: Option<[u8; 32]>,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let bridge_admin_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let owner_associated_info = next_account_info(account_info_iter)?;
    let bridge_associated_info = next_account_info(account_info_iter)?;
    let owner_info = next_account_info(account_info_iter)?;

    let token_program = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;
    let sysvar_info = next_account_info(account_info_iter)?;
    let _associated_program = next_account_info(account_info_iter)?;

    let bridge_admin_key = Pubkey::create_program_address(&[&seeds], &program_id)?;
    if *bridge_admin_info.key != bridge_admin_key {
        return Err(LibError::WrongSeeds.into());
    }

    let bridge_admin: BridgeAdmin = BorshDeserialize::deserialize(&mut bridge_admin_info.data.borrow_mut().as_ref())?;
    if !bridge_admin.is_initialized {
        return Err(LibError::NotInitialized.into());
    }

    verify_commission_charged(bridge_admin_info, sysvar_info, &bridge_admin, lib::TokenType::FT, amount)?;

    if *bridge_associated_info.key !=
        get_associated_token_address(&bridge_admin_key, mint_info.key) {
        return Err(LibError::WrongTokenAccount.into());
    }

    if bridge_associated_info.data.borrow().as_ref().len() == 0 {
        msg!("Creating bridge admin associated account");
        lib::call_create_associated_account(
            owner_info,
            bridge_admin_info,
            mint_info,
            bridge_associated_info,
            rent_info,
            system_program,
            token_program,
        )?;
    }


    if let Some(token_seed) = token_seed {
        let (mint_key, _) = Pubkey::find_program_address(&[token_seed.as_slice()], program_id);
        if mint_key != *mint_info.key {
            return Err(LibError::WrongTokenSeed.into());
        }

        msg!("Burning token");
        call_burn_token(
            owner_associated_info,
            mint_info,
            owner_info,
            amount,
        )?;
    } else {
        msg!("Transferring token");
        call_transfer_token(
            owner_associated_info,
            bridge_associated_info,
            owner_info,
            amount,
            &[],
        )?;
    }

    Ok(())
}

pub fn process_deposit_nft<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    seeds: [u8; 32],
    network: String,
    receiver: String,
    token_seed: Option<[u8; 32]>,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let bridge_admin_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let owner_associated_info = next_account_info(account_info_iter)?;
    let bridge_associated_info = next_account_info(account_info_iter)?;
    let owner_info = next_account_info(account_info_iter)?;

    let token_program = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;
    let sysvar_info = next_account_info(account_info_iter)?;
    let _associated_program = next_account_info(account_info_iter)?;

    let bridge_admin_key = Pubkey::create_program_address(&[&seeds], &program_id)?;
    if *bridge_admin_info.key != bridge_admin_key {
        return Err(LibError::WrongSeeds.into());
    }

    let bridge_admin: BridgeAdmin = BorshDeserialize::deserialize(&mut bridge_admin_info.data.borrow_mut().as_ref())?;
    if !bridge_admin.is_initialized {
        return Err(LibError::NotInitialized.into());
    }

    verify_commission_charged( bridge_admin_info, sysvar_info, &bridge_admin, lib::TokenType::NFT, 1)?;

    if *bridge_associated_info.key !=
        get_associated_token_address(&bridge_admin_key, mint_info.key) {
        return Err(LibError::WrongTokenAccount.into());
    }

    if bridge_associated_info.data.borrow().as_ref().len() == 0 {
        msg!("Creating bridge admin associated account");
        lib::call_create_associated_account(
            owner_info,
            bridge_admin_info,
            mint_info,
            bridge_associated_info,
            rent_info,
            system_program,
            token_program,
        )?;
    }

    if let Some(token_seed) = token_seed {
        let (mint_key, _) = Pubkey::find_program_address(&[token_seed.as_slice()], program_id);
        if mint_key != *mint_info.key {
            return Err(LibError::WrongTokenSeed.into());
        }

        msg!("Burning token");
        call_burn_token(
            owner_associated_info,
            mint_info,
            owner_info,
            1,
        )?;
    } else {
        msg!("Transferring token");
        call_transfer_token(
            owner_associated_info,
            bridge_associated_info,
            owner_info,
            1,
            &[],
        )?;
    }

    Ok(())
}

pub fn process_withdraw_native<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    seeds: [u8; 32],
    signature: [u8; SECP256K1_SIGNATURE_LENGTH],
    recovery_id: u8,
    path: Vec<[u8; 32]>,
    origin: [u8; 32],
    amount: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let bridge_admin_info = next_account_info(account_info_iter)?;
    let owner_info = next_account_info(account_info_iter)?;
    let withdraw_info = next_account_info(account_info_iter)?;

    let system_program = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;

    let bridge_admin_key = Pubkey::create_program_address(&[&seeds], &program_id)?;
    if *bridge_admin_info.key != bridge_admin_key {
        return Err(LibError::WrongSeeds.into());
    }

    let bridge_admin: BridgeAdmin = BorshDeserialize::deserialize(&mut bridge_admin_info.data.borrow_mut().as_ref())?;
    if !bridge_admin.is_initialized {
        return Err(LibError::NotInitialized.into());
    }

    let content = Content::new(
        origin,
        owner_info.key.to_bytes(),
        program_id.to_bytes(),
        Box::new(
            TransferData::new_native_transfer(
                amount,
            ),
        ),
    );
    let root = get_merkle_root(content.hash(), &path)?;

    verify_ecdsa_signature(root.as_slice(), signature.as_slice(), recovery_id, bridge_admin.public_key)?;

    // TODO check rent
    if **bridge_admin_info.try_borrow_lamports()? < amount {
        return Err(LibError::WrongBalance.into());
    }

    let (withdraw_key, bump_seed) = Pubkey::find_program_address(&[origin.as_slice()], program_id);
    if withdraw_key != *withdraw_info.key {
        return Err(LibError::WrongNonce.into());
    }

    // Need to do that before transferring SOls
    msg!("Creating withdraw account");
    lib::call_create_account(
        owner_info,
        withdraw_info,
        rent_info,
        system_program,
        WITHDRAW_SIZE,
        program_id,
        &[origin.as_slice(), &[bump_seed]],
    )?;

    msg!("Transferring token");
    **bridge_admin_info.try_borrow_mut_lamports()? -= amount;
    **owner_info.try_borrow_mut_lamports()? += amount;

    msg!("Initializing withdraw account");
    let mut withdraw: Withdraw = BorshDeserialize::deserialize(&mut withdraw_info.data.borrow_mut().as_ref())?;
    if withdraw.is_initialized {
        return Err(LibError::AlreadyInUse.into());
    }

    withdraw.is_initialized = true;
    withdraw.token_type = lib::TokenType::Native;
    withdraw.origin = origin;
    withdraw.mint = Option::None;
    withdraw.amount = amount;
    withdraw.receiver_address = *owner_info.key;
    withdraw.serialize(&mut *withdraw_info.data.borrow_mut())?;
    msg!("Withdraw account created");
    Ok(())
}

pub fn process_withdraw_ft<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    seeds: [u8; 32],
    signature: [u8; SECP256K1_SIGNATURE_LENGTH],
    recovery_id: u8,
    path: Vec<[u8; 32]>,
    origin: [u8; 32],
    amount: u64,
    token_seed: Option<[u8; 32]>,
    signed_meta: Option<SignedMetadata>,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let bridge_admin_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let metadata_info = next_account_info(account_info_iter)?;
    let owner_info = next_account_info(account_info_iter)?;
    let owner_associated_info = next_account_info(account_info_iter)?;
    let bridge_associated_info = next_account_info(account_info_iter)?;
    let withdraw_info = next_account_info(account_info_iter)?;

    let token_program = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;
    let _metadata_program = next_account_info(account_info_iter)?;
    let _associated_program = next_account_info(account_info_iter)?;

    let bridge_admin_key = Pubkey::create_program_address(&[&seeds], &program_id)?;
    if *bridge_admin_info.key != bridge_admin_key {
        return Err(LibError::WrongSeeds.into());
    }

    let bridge_admin: BridgeAdmin = BorshDeserialize::deserialize(&mut bridge_admin_info.data.borrow_mut().as_ref())?;
    if !bridge_admin.is_initialized {
        return Err(LibError::NotInitialized.into());
    }

    if *metadata_info.key != Metadata::find_pda(mint_info.key).0 {
        return Err(LibError::WrongMetadataAccount.into());
    }

    if let Some(token_seed) = token_seed {
        try_mint_token_with_meta(
            program_id,
            bridge_admin_info,
            token_seed,
            signed_meta,
            mint_info,
            metadata_info,
            owner_info,
            rent_info,
            system_program,
            seeds,
        )?;
    }

    let metadata: mpl_token_metadata::accounts::Metadata = Metadata::from_bytes(&mut metadata_info.data.borrow_mut().as_ref())?;

    let mint: spl_token::state::Mint = Mint::unpack_from_slice(&mut mint_info.data.borrow_mut().as_ref())?;

    let content = Content::new(
        origin,
        owner_info.key.to_bytes(),
        program_id.to_bytes(),
        Box::new(
            TransferData::new_ft_transfer(
                mint_info.key.to_bytes(),
                amount,
                metadata.name.trim_matches(char::from(0)).to_string(),
                metadata.symbol.trim_matches(char::from(0)).to_string(),
                metadata.uri.trim_matches(char::from(0)).to_string(),
                mint.decimals,
            ),
        ),
    );

    verify_ecdsa_signature(get_merkle_root(content.hash(), &path)?.as_slice(), signature.as_slice(), recovery_id, bridge_admin.public_key)?;

    if *bridge_associated_info.key !=
        get_associated_token_address(&bridge_admin_key, mint_info.key) {
        return Err(LibError::WrongTokenAccount.into());
    }

    if bridge_associated_info.data.borrow().as_ref().len() == 0 {
        msg!("Create bridge associated account");
        lib::call_create_associated_account(
            owner_info,
            bridge_admin_info,
            mint_info,
            bridge_associated_info,
            rent_info,
            system_program,
            token_program,
        )?;
    }

    let bridge_associated = spl_token::state::Account::unpack_from_slice(&mut bridge_associated_info.data.borrow_mut().as_ref())?;

    if *owner_associated_info.key !=
        get_associated_token_address(&owner_info.key, mint_info.key) {
        return Err(LibError::WrongTokenAccount.into());
    }

    if owner_associated_info.data.borrow().as_ref().len() == 0 {
        msg!("Create owner associated account");
        lib::call_create_associated_account(
            owner_info,
            owner_info,
            mint_info,
            owner_associated_info,
            rent_info,
            system_program,
            token_program,
        )?;
    }


    if bridge_associated.amount < amount {
        msg!("Minting token to bridge admin");
        call_mint_to(
            mint_info,
            bridge_associated_info,
            bridge_admin_info,
            seeds,
            amount - bridge_associated.amount,
        )?;
    }

    msg!("Transferring token");
    call_transfer_token(
        bridge_associated_info,
        owner_associated_info,
        bridge_admin_info,
        amount,
        &[&[seeds.as_slice()]],
    )?;

    let (withdraw_key, bump_seed) = Pubkey::find_program_address(&[origin.as_slice()], program_id);
    if withdraw_key != *withdraw_info.key {
        return Err(LibError::WrongNonce.into());
    }

    msg!("Creating withdraw account");
    lib::call_create_account(
        owner_info,
        withdraw_info,
        rent_info,
        system_program,
        WITHDRAW_SIZE,
        program_id,
        &[origin.as_slice(), &[bump_seed]],
    )?;

    msg!("Initializing withdraw account");
    let mut withdraw: Withdraw = BorshDeserialize::deserialize(&mut withdraw_info.data.borrow_mut().as_ref())?;
    if withdraw.is_initialized {
        return Err(LibError::AlreadyInUse.into());
    }

    withdraw.is_initialized = true;
    withdraw.token_type = lib::TokenType::FT;
    withdraw.origin = origin;
    withdraw.mint = Option::Some(mint_info.key.clone());
    withdraw.amount = amount;
    withdraw.receiver_address = *owner_info.key;
    withdraw.serialize(&mut *withdraw_info.data.borrow_mut())?;
    msg!("Withdraw account created");
    Ok(())
}

pub fn process_withdraw_nft<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    seeds: [u8; 32],
    signature: [u8; SECP256K1_SIGNATURE_LENGTH],
    recovery_id: u8,
    path: Vec<[u8; 32]>,
    origin: [u8; 32],
    token_seed: Option<[u8; 32]>,
    signed_meta: Option<SignedMetadata>,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let bridge_admin_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let metadata_info = next_account_info(account_info_iter)?;
    let owner_info = next_account_info(account_info_iter)?;
    let owner_associated_info = next_account_info(account_info_iter)?;
    let bridge_associated_info = next_account_info(account_info_iter)?;
    let withdraw_info = next_account_info(account_info_iter)?;

    let token_program = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;
    let _metadata_program = next_account_info(account_info_iter)?;
    let _associated_program = next_account_info(account_info_iter)?;

    let bridge_admin_key = Pubkey::create_program_address(&[&seeds], &program_id)?;
    if *bridge_admin_info.key != bridge_admin_key {
        return Err(LibError::WrongSeeds.into());
    }

    let bridge_admin: BridgeAdmin = BorshDeserialize::deserialize(&mut bridge_admin_info.data.borrow_mut().as_ref())?;
    if !bridge_admin.is_initialized {
        return Err(LibError::NotInitialized.into());
    }

    if *metadata_info.key != Metadata::find_pda(mint_info.key).0 {
        return Err(LibError::WrongMetadataAccount.into());
    }

    if let Some(token_seed) = token_seed {
        try_mint_token_with_meta(
            program_id,
            bridge_admin_info,
            token_seed,
            signed_meta,
            mint_info,
            metadata_info,
            owner_info,
            rent_info,
            system_program,
            seeds,
        )?;
    }

    let metadata: mpl_token_metadata::accounts::Metadata = Metadata::from_bytes(&mut metadata_info.data.borrow_mut().as_ref())?;

    // Default metadata - from token
    let mut name = metadata.name;
    let mut symbol = metadata.symbol;
    let mut uri = metadata.uri;

    let mut collection: Option<[u8; 32]> = None;

    if metadata.collection.is_some() {
        let collection_key = metadata.collection.unwrap().key;

        let collection_metadata_info = next_account_info(account_info_iter)?;
        if *collection_metadata_info.key != Metadata::find_pda(&collection_key).0 {
            return Err(LibError::WrongMetadataAccount.into());
        }

        // If collection exists, use its metadata (name and symbol) instead of token metadata
        let collection_metadata: mpl_token_metadata::accounts::Metadata = Metadata::from_bytes(&mut collection_metadata_info.data.borrow_mut().as_ref())?;
        name = collection_metadata.name;
        symbol = collection_metadata.symbol;
        collection = Some(collection_key.to_bytes())
    }

    let content = Content::new(
        origin,
        owner_info.key.to_bytes(),
        program_id.to_bytes(),
        Box::new(
            TransferData::new_nft_transfer(
                mint_info.key.to_bytes(),
                collection,
                name.trim_matches(char::from(0)).to_string(),
                symbol.trim_matches(char::from(0)).to_string(),
                uri.trim_matches(char::from(0)).to_string(),
            )
        ),
    );

    verify_ecdsa_signature(get_merkle_root(content.hash(), &path)?.as_slice(), signature.as_slice(), recovery_id, bridge_admin.public_key)?;

    if *bridge_associated_info.key !=
        get_associated_token_address(&bridge_admin_key, mint_info.key) {
        return Err(LibError::WrongTokenAccount.into());
    }

    if bridge_associated_info.data.borrow().as_ref().len() == 0 {
        msg!("Create bridge associated account");
        lib::call_create_associated_account(
            owner_info,
            bridge_admin_info,
            mint_info,
            bridge_associated_info,
            rent_info,
            system_program,
            token_program,
        )?;
    }

    let bridge_associated = spl_token::state::Account::unpack_from_slice(&mut bridge_associated_info.data.borrow_mut().as_ref())?;

    if *owner_associated_info.key !=
        get_associated_token_address(&owner_info.key, mint_info.key) {
        return Err(LibError::WrongTokenAccount.into());
    }

    if owner_associated_info.data.borrow().as_ref().len() == 0 {
        msg!("Deposit owner associated account");
        lib::call_create_associated_account(
            owner_info,
            owner_info,
            mint_info,
            owner_associated_info,
            rent_info,
            system_program,
            token_program,
        )?;
    }

    if bridge_associated.amount == 0 {
        msg!("Minting token to bridge admin");
        call_mint_to(
            mint_info,
            bridge_associated_info,
            bridge_admin_info,
            seeds,
            1,
        )?;
    }

    msg!("Transferring token");
    call_transfer_token(
        bridge_associated_info,
        owner_associated_info,
        bridge_admin_info,
        1,
        &[&[seeds.as_slice()]],
    )?;

    let (withdraw_key, bump_seed) = Pubkey::find_program_address(&[origin.as_slice()], program_id);
    if withdraw_key != *withdraw_info.key {
        return Err(LibError::WrongNonce.into());
    }

    msg!("Creating withdraw account");
    lib::call_create_account(
        owner_info,
        withdraw_info,
        rent_info,
        system_program,
        WITHDRAW_SIZE,
        program_id,
        &[origin.as_slice(), &[bump_seed]],
    )?;

    msg!("Initializing withdraw account");
    let mut withdraw: Withdraw = BorshDeserialize::deserialize(&mut withdraw_info.data.borrow_mut().as_ref())?;
    if withdraw.is_initialized {
        return Err(LibError::AlreadyInUse.into());
    }

    withdraw.is_initialized = true;
    withdraw.token_type = lib::TokenType::NFT;
    withdraw.origin = origin;
    withdraw.mint = Option::Some(mint_info.key.clone());
    withdraw.amount = 1;
    withdraw.receiver_address = *owner_info.key;
    withdraw.serialize(&mut *withdraw_info.data.borrow_mut())?;
    msg!("Withdraw account created");
    Ok(())
}

pub fn verify_commission_charged<'a>(
    bridge_admin_info: &AccountInfo<'a>,
    instruction_sysvar_info: &AccountInfo<'a>,
    admin: &BridgeAdmin,
    token: lib::TokenType,
    amount: u64,
) -> ProgramResult {
    let current_index = load_current_index_checked(instruction_sysvar_info)?;
    let commission_instruction = load_instruction_at_checked((current_index - 1) as usize, instruction_sysvar_info)?;

    if commission_instruction.program_id != admin.commission_program {
        return Err(LibError::WrongCommissionProgram.into());
    }

    let commission_key = Pubkey::create_program_address(&[lib::COMMISSION_ADMIN_PDA_SEED.as_bytes(), bridge_admin_info.key.as_ref()], &commission_instruction.program_id)?;
    if commission_key != commission_instruction.accounts[0].pubkey {
        return Err(LibError::WrongCommissionAccount.into());
    }

    let instruction = lib::instructions::commission::CommissionInstruction::try_from_slice(commission_instruction.data.as_slice())?;

    if let lib::instructions::commission::CommissionInstruction::ChargeCommission(args) = instruction {
        if args.deposit_token == token && args.deposit_token_amount == amount {
            return Ok(());
        }
    }

    return Err(LibError::WrongCommissionArguments.into());
}

pub fn process_create_collection<'a>(
    program_id: &'a Pubkey,
    accounts: &'a [AccountInfo<'a>],
    seeds: [u8; 32],
    data: SignedMetadata,
    token_seed: [u8; 32],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let bridge_admin_info = next_account_info(account_info_iter)?;

    let mint_info = next_account_info(account_info_iter)?;
    let bridge_associated_info = next_account_info(account_info_iter)?;
    let metadata_info = next_account_info(account_info_iter)?;

    let payer_info = next_account_info(account_info_iter)?;

    let token_program = next_account_info(account_info_iter)?;
    let _metadata_program = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let _associated_program = next_account_info(account_info_iter)?;

    let bridge_admin_key = Pubkey::create_program_address(&[&seeds], &program_id)?;
    if *bridge_admin_info.key != bridge_admin_key {
        return Err(LibError::WrongSeeds.into());
    }

    let bridge_admin: BridgeAdmin = BorshDeserialize::deserialize(&mut bridge_admin_info.data.borrow_mut().as_ref())?;
    if !bridge_admin.is_initialized {
        return Err(LibError::NotInitialized.into());
    }

    if *bridge_associated_info.key !=
        get_associated_token_address(&bridge_admin_key, mint_info.key) {
        return Err(LibError::WrongTokenAccount.into());
    }

    let (mint_key, _) = Pubkey::find_program_address(&[token_seed.as_slice()], program_id);
    if mint_key != *mint_info.key {
        return Err(LibError::WrongTokenSeed.into());
    }

    msg!("Creating mint account");
    lib::call_create_account(
        payer_info,
        mint_info,
        rent_info,
        system_program,
        Mint::LEN,
        &spl_token::id(),
        &[],
    )?;

    msg!("Initializing mint account");
    call_init_mint(
        mint_info,
        bridge_admin_info,
        rent_info,
        0,
    )?;

    msg!("Crating bridge admin associated account");
    lib::call_create_associated_account(
        payer_info,
        bridge_admin_info,
        mint_info,
        bridge_associated_info,
        rent_info,
        system_program,
        token_program,
    )?;

    msg!("Minting token to bridge admin");
    call_mint_to(
        mint_info,
        bridge_associated_info,
        bridge_admin_info,
        seeds,
        1,
    )?;

    msg!("Creating metadata account");
    call_create_metadata(
        metadata_info,
        mint_info,
        bridge_admin_info,
        payer_info,
        bridge_admin_info,
        rent_info,
        system_program,
        data,
        seeds,
    )?;

    Ok(())
}

fn try_mint_token_with_meta<'a>(
    program_id: &'a Pubkey,
    bridge_admin_info: &AccountInfo<'a>,
    token_seed: [u8; 32],
    signed_meta: Option<SignedMetadata>,
    mint_info: &AccountInfo<'a>,
    metadata_info: &AccountInfo<'a>,
    owner_info: &AccountInfo<'a>,
    rent_info: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    seeds: [u8; 32],
) -> ProgramResult {
    let (mint_key, bump_seed) = Pubkey::find_program_address(&[token_seed.as_slice()], program_id);
    if mint_key != *mint_info.key {
        return Err(LibError::WrongTokenSeed.into());
    }

    let signed_meta = {
        if signed_meta.is_none() {
            return Err(LibError::NoTokenMeta.into());
        }

        Ok::<SignedMetadata, LibError>(signed_meta.unwrap())
    }?;

    if mint_info.data.borrow().as_ref().len() == 0 {
        msg!("Creating mint account");
        lib::call_create_account(
            owner_info,
            mint_info,
            rent_info,
            system_program,
            Mint::LEN,
            &spl_token::id(),
            &[token_seed.as_slice(), &[bump_seed]],
        )?;

        msg!("Initializing mint account");
        call_init_mint(
            mint_info,
            bridge_admin_info,
            rent_info,
            signed_meta.decimals,
        )?;

        msg!("Creating metadata account");
        call_create_metadata(
            metadata_info,
            mint_info,
            bridge_admin_info,
            owner_info,
            bridge_admin_info,
            rent_info,
            system_program,
            signed_meta,
            seeds,
        )?;
    }

    Ok(())
}


fn call_burn_token<'a>(
    associated_info: &AccountInfo<'a>,
    mint_info: &AccountInfo<'a>,
    authority_info: &AccountInfo<'a>,
    amount: u64,
) -> ProgramResult {
    let burn_tokens_instruction = burn(
        &spl_token::id(),
        associated_info.key,
        mint_info.key,
        authority_info.key,
        &[],
        amount,
    )?;

    invoke(
        &burn_tokens_instruction,
        &[
            associated_info.clone(),
            mint_info.clone(),
            authority_info.clone(),
        ],
    )
}

fn call_transfer_token<'a>(
    from: &AccountInfo<'a>,
    to: &AccountInfo<'a>,
    authority: &AccountInfo<'a>,
    amount: u64,
    signers_seeds: &[&[&[u8]]],
) -> ProgramResult {
    let transfer_tokens_instruction = transfer(
        &spl_token::id(),
        from.key,
        to.key,
        authority.key,
        &[],
        amount,
    )?;

    invoke_signed(
        &transfer_tokens_instruction,
        &[
            from.clone(),
            to.clone(),
            authority.clone(),
        ],
        signers_seeds,
    )
}

fn call_mint_to<'a>(
    mint: &AccountInfo<'a>,
    account: &AccountInfo<'a>,
    owner: &AccountInfo<'a>,
    seeds: [u8; 32],
    amount: u64,
) -> ProgramResult {
    let mint_to_instruction = mint_to(
        &spl_token::id(),
        mint.key,
        account.key,
        owner.key,
        &[],
        amount,
    )?;

    invoke_signed(
        &mint_to_instruction,
        &[
            mint.clone(),
            account.clone(),
            owner.clone(),
        ],
        &[&[&seeds]],
    )
}

fn call_init_mint<'a>(
    mint: &AccountInfo<'a>,
    mint_authority: &AccountInfo<'a>,
    rent: &AccountInfo<'a>,
    decimals: u8,
) -> ProgramResult {
    let init_mint_instruction = initialize_mint(
        &spl_token::id(),
        mint.key,
        mint_authority.key,
        None,
        decimals,
    )?;

    invoke(
        &init_mint_instruction,
        &[
            mint.clone(),
            rent.clone(),
        ],
    )
}

fn call_create_metadata<'a>(
    metadata_account: &AccountInfo<'a>,
    mint: &AccountInfo<'a>,
    mint_authority: &AccountInfo<'a>,
    payer: &AccountInfo<'a>,
    update_authority: &AccountInfo<'a>,
    rent: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    data: SignedMetadata,
    seeds: [u8; 32],
) -> ProgramResult {
    /*
    let create_metadata_instruction = create_metadata_accounts_v3(
        mpl_token_metadata::ID,
        *metadata_account.key,
        *mint.key,
        *mint_authority.key,
        *payer.key,
        *mint_authority.key,
        data.name,
        data.symbol,
        data.uri,
        None,
        0,
        true,
        true,
        None,
        None,
        None,
    );
    */
    let args = CreateMetadataAccountV3InstructionArgs{
        data: DataV2{
            name: data.name,
            symbol: data.symbol,
            uri: data.uri,
            seller_fee_basis_points: 0,
            creators: None,
            collection: None,
            uses: None,
        },
        is_mutable: false,
        collection_details: None,
    };
   
    let metadata_account_v3 = CreateMetadataAccountV3{
        metadata: *metadata_account.key,
        mint: *mint.key,
        mint_authority: *mint_authority.key,
        payer: *payer.key,
        update_authority: (*mint_authority.key, true),
        system_program: g_system_program::ID,
        rent: None,
    };


    let create_metadata_instruction = metadata_account_v3.instruction(args);


    invoke_signed(
        &create_metadata_instruction,
        &[
            metadata_account.clone(),
            mint.clone(),
            mint_authority.clone(),
            payer.clone(),
            update_authority.clone(),
            rent.clone(),
            system_program.clone(),
        ],
        &[&[&seeds]],
    )
}
