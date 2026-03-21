use std::io::{self, Write};
use p2p_core::{JoinForm, NetId, Password};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let name = prompt("My name: ")?;
    let net_id = prompt("Network ID: ")?;
    let password = prompt("Password: ")?;
    let is_server = prompt("Is server? (y/n): ")?.trim().eq_ignore_ascii_case("y");

    println!("Joining network...");
    let net = p2p_core::join_p2p_net(JoinForm {
        net_id: NetId(net_id),
        pw: Password(password),
        my_name: name,
        is_server,
    })
    .await?;

    println!("Joined. My EndpointId: {}", net.my_id());

    loop {
        let peers = net.get_peers();
        println!("--- peers ({}) ---", peers.len());
        for p in &peers {
            println!("  {} [{}]", p.name, if p.is_server { "server" } else { "client" });
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

fn prompt(msg: &str) -> anyhow::Result<String> {
    print!("{}", msg);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}
