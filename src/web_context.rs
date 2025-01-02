use std::collections::HashMap;

use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub struct WebContext {
    pub songs: HashMap<SongId, SongData>,
    pub web_socket: WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
}

pub struct SongData {
    hash: SongId,
}

pub struct SongId(pub String);
