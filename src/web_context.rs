use std::collections::HashMap;

use bytes::{Buf, Bytes, BytesMut};
use futures::TryStreamExt;
use prost::Message;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::proto::{
    packets::{
        AllSongs, Command, DownloadSong, NowPlaying, NowPlayingUpdate, PlaySong, PreviewSong,
        SongList,
    },
    CommandType, PacketType, PartyPacket,
};

pub struct WebContext {
    pub songs: HashMap<SongId, SongData>,
    pub socket: TcpStream, //WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
}

pub struct SongData {
    hash: SongId,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SongId(pub String);

impl WebContext {
    pub async fn read_loop(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut buf: BytesMut = BytesMut::with_capacity(1024);

        loop {
            // buffer the size of UTF8 "moon"
            let mut header = [0u8; 4];
            self.socket.read_exact(&mut header).await?;

            // continue? restart loop
            if header != "moon".as_bytes() {
                return Err("Invalid header".into());
            }

            let packet_type: PacketType = self.socket.read_i32().await?.into();

            let len = self.socket.read_u64().await? as usize;
            // resize to read the amount we need
            buf.resize(len, 0);

            let buffer = self.socket.read_exact(&mut buf).await?;
            if buffer < len {
                return Err("Failed to read the full buffer".into());
            }

            self.parse_packet(packet_type, buf.copy_to_bytes(len))?;
        }

        Ok(())
    }

    pub fn parse_packet(
        &mut self,
        packet_type: PacketType,
        data: Bytes,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match packet_type {
            PacketType::SongList => {
                let song_list = SongList::decode(data)?;
                self.songs = song_list
                    .levels
                    .into_iter()
                    .map(|song| {
                        (
                            SongId(song.level_id.clone()),
                            SongData {
                                hash: SongId(song.level_id),
                            },
                        )
                    })
                    .collect();
            }
            PacketType::Command => {
                let command = Command::decode(data)?;
                let command_type = CommandType::from(command.command_type);
                match command_type {
                    CommandType::CommandTypeHeartbeat => {
                        // heartbeat
                    }
                    CommandType::CommandTypeReturnToMenu => {
                        // return to menu
                    }
                    _ => {}
                }
            }
            PacketType::NowPlaying => {
                let now_playing = NowPlaying::decode(data)?;
            }
            PacketType::NowPlayingUpdate => {
                let now_playing_update = NowPlayingUpdate::decode(data)?;
            }
            PacketType::PlaySong => {
                let play_song = PlaySong::decode(data)?;
            }
            PacketType::PreviewSong => {
                let preview_song = PreviewSong::decode(data)?;
            }
            PacketType::DownloadSong => {
                let download_song = DownloadSong::decode(data)?;
            }
            PacketType::AllSongs => {
                let all_songs = AllSongs::decode(data)?;
            }
            _ => {}
        }

        Ok(())
    }

    pub async fn write_packet(
        &mut self,
        packet: impl PartyPacket,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let packet_type = packet.get_type();
        let data = packet.encode_to_vec();

        self.socket.write_all("moon".as_bytes()).await?;
        self.socket.write_i32(packet_type as i32).await?;
        self.socket.write_u64(data.len() as u64).await?;
        self.socket.write_all(&data).await?;

        Ok(())
    }
}
