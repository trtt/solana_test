use std::{collections::HashMap, env, fs::File, io::BufReader, sync::Arc};

use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::{CommitmentConfig, CommitmentLevel},
    native_token::LAMPORTS_PER_SOL,
    signature::read_keypair_file,
    signer::Signer,
    system_instruction,
    transaction::Transaction,
};
use yellowstone_grpc_client::{ClientTlsConfig, GeyserGrpcBuilder};
use yellowstone_grpc_proto::geyser::{
    SubscribeRequest, SubscribeRequestFilterBlocksMeta, subscribe_update::UpdateOneof,
};

#[derive(Debug, Deserialize)]
struct Config {
    grpc_endpoint: String,
    grpc_token: String,
    sender_keypair: String,
    recipient: String,
    sol: f64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = env::args().nth(1).expect("Usage: blocks <config.yaml>");

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let config: Config = serde_yaml::from_reader(reader)?;

    let rpc_client = RpcClient::new_with_commitment(
        "https://api.devnet.solana.com".to_string(),
        CommitmentConfig::confirmed(),
    );
    let rpc_client = Arc::new(rpc_client);

    let mut grpc_client = GeyserGrpcBuilder::from_shared(config.grpc_endpoint)?
        .x_token(Some(config.grpc_token))?
        .tls_config(ClientTlsConfig::new().with_native_roots())?
        .connect()
        .await?;

    let (mut sub, mut updates) = grpc_client.subscribe().await?;

    let commitment = CommitmentLevel::Processed;
    sub.send(SubscribeRequest {
        slots: HashMap::new(),
        accounts: HashMap::new(),
        transactions: HashMap::new(),
        transactions_status: HashMap::new(),
        entry: HashMap::new(),
        blocks: HashMap::new(),
        blocks_meta: HashMap::from([("".to_owned(), SubscribeRequestFilterBlocksMeta {})]),
        commitment: Some(commitment as i32),
        accounts_data_slice: vec![],
        ping: None,
        from_slot: None,
    })
    .await?;

    let sender_keypair = read_keypair_file(config.sender_keypair)
        .map_err(|err| anyhow::anyhow!("Can't read keypair file: {err}"))?;
    let sender_keypair = Arc::new(sender_keypair);
    let recipient = config.recipient.parse()?;
    let lamports = (config.sol * LAMPORTS_PER_SOL as f64).round() as u64;
    let transfer = |blockhash: solana_sdk::hash::Hash| {
        let rpc_client = rpc_client.clone();
        let sender_keypair = sender_keypair.clone();
        async move {
            // NOTE: can't use `blockhash` since I'm sending tx on testnet :(
            let recent_blockhash = match rpc_client.get_latest_blockhash().await {
                Ok(hash) => hash,
                Err(err) => {
                    eprintln!("get_latest_blockhash error: {err}");
                    return;
                }
            };

            let tx = Transaction::new_signed_with_payer(
                &[system_instruction::transfer(
                    &sender_keypair.pubkey(),
                    &recipient,
                    lamports,
                )],
                Some(&sender_keypair.pubkey()),
                &[&sender_keypair],
                recent_blockhash,
            );
            let res = rpc_client.send_and_confirm_transaction(&tx).await;
            match res {
                Ok(signature) => {
                    println!("send success for block {blockhash}: tx {signature}");
                }
                Err(err) => {
                    eprintln!("send error for block {blockhash}: {err}");
                }
            }
        }
    };

    while let Some(update) = updates.next().await {
        match update {
            Ok(msg) => {
                if let Some(UpdateOneof::BlockMeta(block)) = msg.update_oneof {
                    // example filter condition
                    if block.slot % 10 == 5 {
                        let blockhash = block.blockhash.parse()?;
                        println!("detected block {blockhash}, sending...");
                        tokio::spawn(transfer(blockhash));
                    }
                }
            }
            Err(error) => {
                eprintln!("stream error: {error:?}");
                break;
            }
        }
    }

    Ok(())
}
