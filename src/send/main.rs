use std::{env, fs::File, io::BufReader, sync::Arc, time::Instant};

use anyhow::Result;
use serde::Deserialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, read_keypair_file},
    signer::Signer,
    system_instruction,
    transaction::Transaction,
};
use tokio::task;

#[derive(Debug, Deserialize)]
struct TransferPairRead {
    sender_keypair: String,
    recipient: String,
    lamports: u64,
}

#[derive(Debug)]
struct TransferPair {
    sender_keypair: Keypair,
    recipient: Pubkey,
    lamports: u64,
}

impl TryFrom<TransferPairRead> for TransferPair {
    type Error = anyhow::Error;

    fn try_from(value: TransferPairRead) -> anyhow::Result<Self> {
        let TransferPairRead {
            sender_keypair,
            recipient,
            lamports,
        } = value;
        Ok(Self {
            sender_keypair: read_keypair_file(&sender_keypair)
                .map_err(|err| anyhow::anyhow!("Can't read keypair file: {err}"))?,
            recipient: recipient.parse()?,
            lamports,
        })
    }
}

#[derive(Debug, Deserialize)]
struct Config {
    pairs: Vec<TransferPairRead>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = env::args().nth(1).expect("Usage: send <config.yaml>");

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let config: Config = serde_yaml::from_reader(reader)?;

    let client = RpcClient::new_with_commitment(
        "https://api.devnet.solana.com".to_string(),
        CommitmentConfig::confirmed(),
    );
    let client = Arc::new(client);

    let recent_blockhash = client.get_latest_blockhash().await?;

    let mut handles = Vec::new();

    let start = Instant::now();

    let pairs: Vec<TransferPair> = config
        .pairs
        .into_iter()
        .map(|pair| pair.try_into())
        .collect::<Result<_, _>>()?;

    for pair in pairs {
        let client = client.clone();
        handles.push(task::spawn(async move {
            let start = Instant::now();

            let tx = Transaction::new_signed_with_payer(
                &[system_instruction::transfer(
                    &pair.sender_keypair.pubkey(),
                    &pair.recipient,
                    pair.lamports,
                )],
                Some(&pair.sender_keypair.pubkey()),
                &[&pair.sender_keypair],
                recent_blockhash,
            );

            let signature = client.send_and_confirm_transaction(&tx).await;

            let duration = start.elapsed();

            (pair.sender_keypair.pubkey(), duration, signature)
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await?);
    }
    let total_duration = start.elapsed();

    println!("Total time: {:?}\n", total_duration);

    results.sort_unstable_by_key(|(_from, duration, _sig)| std::cmp::Reverse(*duration));

    for (from, duration, sig) in results {
        match sig {
            Ok(sig) => {
                println!("took {duration:?} from {from} success: {sig}");
            }
            Err(e) => {
                println!("took {duration:?} from {from} error: {e}");
            }
        }
    }

    Ok(())
}
