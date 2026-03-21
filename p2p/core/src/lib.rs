mod h3_iroh;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::Result;
use bytes::Bytes;
use iroh::{endpoint::presets, Endpoint, EndpointAddr, EndpointId};
use pbkdf2::pbkdf2_hmac;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

// ── 공개 타입 ────────────────────────────────────────────────────────────────

pub struct NetId(pub String);
pub struct Password(pub String);

pub type NodeAddr = EndpointAddr;

pub struct Node {
    endpoint: Endpoint,
    _accept_task: tokio::task::JoinHandle<()>,
}

impl Node {
    pub fn id(&self) -> EndpointId { self.endpoint.id() }
    pub fn addr(&self) -> NodeAddr { self.endpoint.addr() }
}

// iroh QUIC 연결 핸들 - 데이터 송수신에 사용
pub struct Connection(iroh::endpoint::Connection);

impl Connection {
    pub async fn send(&self, data: &[u8]) -> Result<()> {
        let mut send = self.0.open_uni().await?;
        send.write_all(data).await?;
        send.finish()?;
        Ok(())
    }

    pub async fn recv(&self) -> Result<Vec<u8>> {
        let mut recv = self.0.accept_uni().await?;
        let data = recv.read_to_end(usize::MAX).await?;
        Ok(data)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub addr: NodeAddr,
    pub name: String,
    pub is_server: bool,
}

pub struct JoinForm {
    pub net_id: NetId,
    pub pw: Password,
    pub my_name: String,
    pub is_server: bool,
}

// ── ALPN 식별자 ──────────────────────────────────────────────────────────────

const COORD_ALPN: &[u8] = b"cv-coord/0";
pub const DATA_ALPN: &[u8] = b"cv-data/0";
const HTTP_ALPN: &[u8] = b"h3";

const PKARR_RELAY: &str = "https://dns.iroh.link/pkarr";

// ── 메시지 타입 ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
enum ToCoord {
    Register(PeerInfo),
    Heartbeat,
}

#[derive(Serialize, Deserialize)]
enum ToPeer {
    Ack(Vec<PeerInfo>),
    Broadcast(Vec<PeerInfo>),
}

// ── 코디네이터 상태 ──────────────────────────────────────────────────────────

struct PeerSlot {
    info: PeerInfo,
    // None = 코디네이터 자신 (연결 없음, 타임아웃 없음)
    conn: Option<iroh::endpoint::Connection>,
    last_seen: Option<Instant>,
}

type PeerMap = Arc<Mutex<HashMap<EndpointId, PeerSlot>>>;

// ── 내부 구현 ─────────────────────────────────────────────────────────────────

fn derive_network_keypair(net_id: &str, pw: &str) -> pkarr::Keypair {
    let mut secret = [0u8; 32];
    pbkdf2_hmac::<Sha256>(pw.as_bytes(), net_id.as_bytes(), 100_000, &mut secret);
    pkarr::Keypair::from_secret_key(&secret)
}

async fn read_coordinator_id(keypair: &pkarr::Keypair) -> Option<EndpointId> {
    let client = reqwest::Client::builder().build().ok()?;
    let z32 = keypair.public_key().to_z32();
    let url = format!("{PKARR_RELAY}/{z32}");

    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }

    let payload = resp.bytes().await.ok()?;
    let packet = pkarr::SignedPacket::from_relay_payload(&keypair.public_key(), &payload).ok()?;

    for record in packet.all_resource_records() {
        if let pkarr::dns::rdata::RData::TXT(txt) = &record.rdata {
            if let Ok(id_str) = String::try_from(txt.clone()) {
                if let Ok(id) = id_str.trim().parse::<EndpointId>() {
                    return Some(id);
                }
            }
        }
    }
    None
}

async fn publish_coordinator_id(keypair: &pkarr::Keypair, my_id: EndpointId) -> Result<()> {
    let client = reqwest::Client::builder().build()?;
    let z32 = keypair.public_key().to_z32();
    let url = format!("{PKARR_RELAY}/{z32}");
    let id_str = my_id.to_string();

    let txt = pkarr::dns::rdata::TXT::try_from(id_str.as_str())
        .map_err(|e| anyhow::anyhow!("TXT error: {e}"))?;
    let name = pkarr::dns::Name::new(".")
        .map_err(|e| anyhow::anyhow!("Name error: {e}"))?;

    let signed = pkarr::SignedPacket::builder()
        .txt(name, txt.into_owned(), 300)
        .sign(keypair)
        .map_err(|e| anyhow::anyhow!("sign error: {e}"))?;

    let payload: Bytes = signed.to_relay_payload();
    client
        .put(&url)
        .header("Content-Type", "application/octet-stream")
        .body(payload.to_vec())
        .send()
        .await?;

    Ok(())
}

async fn send_to_peer(conn: &iroh::endpoint::Connection, msg: &ToPeer) -> Result<()> {
    let data = serde_json::to_vec(msg)?;
    let mut send = conn.open_uni().await?;
    send.write_all(&data).await?;
    send.finish()?;
    Ok(())
}

async fn send_to_coord(conn: &iroh::endpoint::Connection, msg: &ToCoord) -> Result<()> {
    let data = serde_json::to_vec(msg)?;
    let mut send = conn.open_uni().await?;
    send.write_all(&data).await?;
    send.finish()?;
    Ok(())
}

fn peer_map_to_list(peers: &PeerMap) -> Vec<PeerInfo> {
    peers.lock().unwrap().values().map(|s| s.info.clone()).collect()
}

// conn이 있는 슬롯에만 브로드캐스트 (except_id 제외)
async fn broadcast_to_all_except(peers: &PeerMap, list: &[PeerInfo], except_id: Option<EndpointId>) {
    let conns: Vec<iroh::endpoint::Connection> = {
        let map = peers.lock().unwrap();
        map.iter()
            .filter(|(id, slot)| Some(**id) != except_id && slot.conn.is_some())
            .map(|(_, slot)| slot.conn.clone().unwrap())
            .collect()
    };
    for conn in conns {
        if let Err(e) = send_to_peer(&conn, &ToPeer::Broadcast(list.to_vec())).await {
            eprintln!("[coord] broadcast error: {e}");
        }
    }
}

// 코디네이터: 개별 피어 연결 처리
async fn handle_coord_conn(conn: iroh::endpoint::Connection, peers: PeerMap) {
    let peer_id = conn.remote_id();

    loop {
        let mut recv = match conn.accept_uni().await {
            Ok(r) => r,
            Err(_) => break,
        };
        let data = match recv.read_to_end(64 * 1024).await {
            Ok(d) => d,
            Err(_) => break,
        };
        let msg: ToCoord = match serde_json::from_slice(&data) {
            Ok(m) => m,
            Err(e) => { eprintln!("[coord] parse error: {e}"); continue; }
        };

        match msg {
            ToCoord::Register(info) => {
                let list = {
                    let mut map = peers.lock().unwrap();
                    map.insert(peer_id, PeerSlot {
                        info,
                        conn: Some(conn.clone()),
                        last_seen: Some(Instant::now()),
                    });
                    map.values().map(|s| s.info.clone()).collect::<Vec<_>>()
                };
                if let Err(e) = send_to_peer(&conn, &ToPeer::Ack(list.clone())).await {
                    eprintln!("[coord] ack error: {e}");
                }
                broadcast_to_all_except(&peers, &list, Some(peer_id)).await;
            }
            ToCoord::Heartbeat => {
                if let Some(slot) = peers.lock().unwrap().get_mut(&peer_id) {
                    slot.last_seen = Some(Instant::now());
                }
            }
        }
    }

    let removed = peers.lock().unwrap().remove(&peer_id).is_some();
    if removed {
        let list = peer_map_to_list(&peers);
        broadcast_to_all_except(&peers, &list, None).await;
    }
}

// 코디네이터: 연결 수신 루프
async fn coord_accept_loop(endpoint: Endpoint, peers: PeerMap) {
    loop {
        let Some(incoming) = endpoint.accept().await else { break };
        let peers = peers.clone();
        tokio::spawn(async move {
            let mut accepting = match incoming.accept() {
                Ok(a) => a,
                Err(e) => { eprintln!("[coord] accept error: {e}"); return; }
            };
            let alpn = match accepting.alpn().await {
                Ok(a) => a,
                Err(e) => { eprintln!("[coord] alpn error: {e}"); return; }
            };
            if alpn != COORD_ALPN { return; }
            let conn = match accepting.await {
                Ok(c) => c,
                Err(e) => { eprintln!("[coord] conn error: {e}"); return; }
            };
            handle_coord_conn(conn, peers).await;
        });
    }
}

// last_seen이 있는 슬롯만 타임아웃 검사 (코디네이터 자신 제외)
async fn heartbeat_checker(peers: PeerMap) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let now = Instant::now();

        let timed_out: Vec<EndpointId> = {
            let map = peers.lock().unwrap();
            map.iter()
                .filter(|(_, slot)| {
                    slot.last_seen.map_or(false, |t| now.duration_since(t).as_secs() >= 5)
                })
                .map(|(id, _)| *id)
                .collect()
        };

        if !timed_out.is_empty() {
            {
                let mut map = peers.lock().unwrap();
                for id in &timed_out {
                    map.remove(id);
                }
            }
            let list = peer_map_to_list(&peers);
            broadcast_to_all_except(&peers, &list, None).await;
        }
    }
}

// 피어: 코디네이터로부터 Ack/Broadcast 수신 루프
async fn peer_recv_loop(conn: iroh::endpoint::Connection, peers: Arc<Mutex<Vec<PeerInfo>>>) {
    loop {
        let mut recv = match conn.accept_uni().await {
            Ok(r) => r,
            Err(_) => break,
        };
        let data = match recv.read_to_end(64 * 1024).await {
            Ok(d) => d,
            Err(_) => break,
        };
        let msg: ToPeer = match serde_json::from_slice(&data) {
            Ok(m) => m,
            Err(e) => { eprintln!("[peer] parse error: {e}"); continue; }
        };
        let list = match msg {
            ToPeer::Ack(list) | ToPeer::Broadcast(list) => list,
        };
        *peers.lock().unwrap() = list;
    }
}

// 피어: 4초마다 Heartbeat 전송
async fn peer_heartbeat_loop(conn: iroh::endpoint::Connection) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        if let Err(e) = send_to_coord(&conn, &ToCoord::Heartbeat).await {
            eprintln!("[peer] heartbeat error: {e}");
            break;
        }
    }
}

async fn create_endpoint() -> Result<Endpoint> {
    let endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![COORD_ALPN.to_vec(), DATA_ALPN.to_vec()])
        .bind()
        .await?;
    Ok(endpoint)
}

// ── 공개 API ─────────────────────────────────────────────────────────────────

enum Peers {
    Coord(PeerMap),
    Peer(Arc<Mutex<Vec<PeerInfo>>>),
}

pub struct P2PNet {
    node: Node,
    peers: Peers,
}

impl P2PNet {
    pub fn my_id(&self) -> EndpointId {
        self.node.id()
    }

    pub fn my_addr(&self) -> NodeAddr {
        self.node.addr()
    }

    pub fn get_peers(&self) -> Vec<PeerInfo> {
        match &self.peers {
            Peers::Coord(map) => peer_map_to_list(map),
            Peers::Peer(list) => list.lock().unwrap().clone(),
        }
    }

    pub async fn connect_udp(&self, addr: NodeAddr) -> Result<Connection> {
        let conn = self.node.endpoint.connect(addr, DATA_ALPN).await?;
        Ok(Connection(conn))
    }

    pub async fn request_http(&self, addr: NodeAddr, path: &str) -> Result<Vec<u8>> {
        let conn = self.node.endpoint.connect(addr, HTTP_ALPN).await?;
        let h3_conn = h3_iroh::Connection::new(conn);
        let (mut driver, mut send_request) = h3::client::new(h3_conn).await?;
        tokio::spawn(async move { let _ = driver.wait_idle().await; });

        let req = http::Request::builder()
            .method(http::Method::GET)
            .uri(path)
            .body(())?;
        let mut stream = send_request.send_request(req).await?;
        stream.finish().await?;

        let _resp = stream.recv_response().await?;
        let mut body = Vec::new();
        while let Some(chunk) = stream.recv_data().await? {
            use bytes::Buf;
            body.extend_from_slice(chunk.chunk());
        }
        Ok(body)
    }
}

pub async fn join_p2p_net(form: JoinForm) -> Result<P2PNet> {
    let endpoint = create_endpoint().await?;
    endpoint.online().await;

    let keypair = derive_network_keypair(&form.net_id.0, &form.pw.0);
    let coord_id = read_coordinator_id(&keypair).await;

    match coord_id {
        None => {
            println!("[join] no coordinator found, becoming coordinator");
            publish_coordinator_id(&keypair, endpoint.id()).await?;

            let my_info = PeerInfo {
                addr: endpoint.addr(),
                name: form.my_name,
                is_server: form.is_server,
            };

            let peers: PeerMap = Arc::new(Mutex::new(HashMap::new()));
            // 코디네이터 자신을 목록에 한 번 삽입 (conn/last_seen = None)
            peers.lock().unwrap().insert(endpoint.id(), PeerSlot {
                info: my_info,
                conn: None,
                last_seen: None,
            });

            let accept_handle = tokio::spawn(coord_accept_loop(endpoint.clone(), peers.clone()));
            tokio::spawn(heartbeat_checker(peers.clone()));

            let node = Node { endpoint, _accept_task: accept_handle };
            Ok(P2PNet { node, peers: Peers::Coord(peers) })
        }
        Some(coord_id) => {
            println!("[join] coordinator found: {coord_id}");
            let coord_addr: NodeAddr = coord_id.into();
            let conn = endpoint.connect(coord_addr, COORD_ALPN).await?;

            let my_info = PeerInfo {
                addr: endpoint.addr(),
                name: form.my_name,
                is_server: form.is_server,
            };
            send_to_coord(&conn, &ToCoord::Register(my_info)).await?;

            let peers: Arc<Mutex<Vec<PeerInfo>>> = Arc::new(Mutex::new(Vec::new()));
            tokio::spawn(peer_recv_loop(conn.clone(), peers.clone()));
            tokio::spawn(peer_heartbeat_loop(conn));

            let node = Node { endpoint, _accept_task: tokio::spawn(async {}) };
            Ok(P2PNet { node, peers: Peers::Peer(peers) })
        }
    }
}
