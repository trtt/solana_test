use std::{collections::HashMap, env, fs::File, io::BufReader, sync::Arc};

use serde::Deserialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use tokio::task;

#[derive(Debug, Deserialize)]
struct Config {
    addresses: Vec<String>,
}

async fn get_balance(client: Arc<RpcClient>, address: Pubkey) -> anyhow::Result<u64> {
    Ok(client.get_balance(&address).await?)
}

async fn get_balances(
    client: Arc<RpcClient>,
    addresses: impl IntoIterator<Item = Pubkey>,
) -> anyhow::Result<HashMap<Pubkey, u64>> {
    let mut handles = vec![];
    for address in addresses {
        let client = Arc::clone(&client);
        handles.push((address, task::spawn(get_balance(client, address))));
    }

    let mut out = HashMap::new();
    for (address, handle) in handles {
        let balance = handle.await??;
        out.insert(address, balance);
    }
    Ok(out)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = env::args().nth(1).expect("Usage: balance <config.yaml>");

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let config: Config = serde_yaml::from_reader(reader)?;
    let addresses: Vec<Pubkey> = config
        .addresses
        .into_iter()
        .map(|addr| addr.parse::<Pubkey>())
        .collect::<Result<_, _>>()?;

    let client = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));

    let balances = get_balances(client, addresses).await?;
    for (k, v) in balances {
        println!("{k}: {v}");
    }

    Ok(())
}
