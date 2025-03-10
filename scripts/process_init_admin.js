const {
    clusterApiUrl,
    Connection,
    Keypair,
    LAMPORTS_PER_SOL,
    PublicKey,
    sendAndConfirmTransaction,
    SystemProgram,
    Transaction,
    TransactionInstruction,
  } = require('@solana/web3.js');

import { serialize, deserialize, deserializeUnchecked } from "borsh";


const fs = require('fs');
const path = require('path');


// 定义与 Rust 中 BridgeInstruction 对应的 JavaScript 枚举
const BridgeInstruction = {
    InitializeAdmin: 0,
  };
  
// 定义 InitializeAdminArgs 结构
class InitializeAdminArgs {
    constructor(publicKey, seeds, commissionProgram) {
        this.publicKey = publicKey;
        this.seeds = seeds;
        this.commissionProgram = commissionProgram;
    }
}

// 定义 Borsh 构造函数
const bridgeInstructionSchema = new Map([
    [
      BridgeInstruction,
      {
        kind: "enum",
        variants: [
          {
            name: "InitializeAdmin",
            kind: "struct",
            fields: [
              ["publicKey", [65, "u8"]],
              ["seeds", [32, "u8"]],
              ["commissionProgram", "pubkey"],
            ],
          },
        ],
      },
    ],
  ]);
  

async function main() {
    console.log(__dirname)
    const signer = Keypair.fromSecretKey(Uint8Array.from(
        JSON.parse(
            fs.readFileSync(path.join(__dirname, './id.json'), "utf-8")
        )
        ));
    console.log(signer.publicKey.toString())
    const connection = new Connection('https://api-testnet.ogchain.io', 'confirmed');
    const latestBlockHash = await connection.getLatestBlockhash();
    console.log(latestBlockHash)
    const programId = new PublicKey('4UkVzysY6CJpwzeX5ao5DMyzPy44HaJTavK2QKtJXepL');
    const [pda, bump] =  PublicKey.findProgramAddressSync(
        [Buffer.from('bridge_admin_info'), signer.publicKey.toBuffer()],
        programId
      );
    
    console.log(pda.toString())

    const data = Buffer.from(serialize(payloadSchema, mint));

    const createPDAIx = new TransactionInstruction({
        programId: programId,
        data: data,//Buffer.from(Uint8Array.of(bump)),
        keys: [
          {
            isSigner: true,
            isWritable: true,
            pubkey: PAYER_KEYPAIR.publicKey,
          },
          {
            isSigner: false,
            isWritable: true,
            pubkey: pda,
          },
          {
            isSigner: false,
            isWritable: false,
            pubkey: SystemProgram.programId,
          },
        ],
      });
}


main().catch((error) => {
    console.log(error)
    process.exitCode = 1;
})
