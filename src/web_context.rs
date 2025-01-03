use std::collections::HashMap;

use bs_cordl::{
    GlobalNamespace::{
        BeatmapCharacteristicSO, BeatmapDifficulty, BeatmapLevel, GameplayModifiers,
        PracticeSettings, SoloFreePlayFlowCoordinator,
    },
    System::Threading::CancellationTokenSource,
};
use bytes::{Buf, Bytes, BytesMut};
use futures::TryStreamExt;
use prost::Message;
use quest_hook::libil2cpp::Gc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::info;

use crate::proto::{
    items::{
        gameplay_modifiers::{EnabledObstacleType, EnergyType, SongSpeed},
        PreviewBeatmapLevel,
    },
    packets::{
        AllSongs, Command, DownloadSong, NowPlaying, NowPlayingUpdate, PlaySong, PreviewSong,
        SongList,
    },
    CommandType, PacketType, PartyPacket,
};

pub struct WebContext {
    pub songs: HashMap<SongId, SongData>,
    pub level_cancellation_token_source: Option<Gc<CancellationTokenSource>>,
    pub get_status_cancellation_token_source: Option<Gc<CancellationTokenSource>>,
    pub flow: Option<Gc<SoloFreePlayFlowCoordinator>>,
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
                    CommandType::Heartbeat => {
                        // heartbeat
                    }
                    CommandType::ReturnToMenu => {
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

    pub fn convert_practice(
        practice_settings: &PracticeSettings,
    ) -> quest_hook::libil2cpp::Result<Gc<PracticeSettings>> {
        PracticeSettings::New_f32_f32_2(0.0, practice_settings._songSpeedMul / 10000.0)
    }

    pub fn convert_modifiers(
        mods: &GameplayModifiers,
    ) -> quest_hook::libil2cpp::Result<Gc<GameplayModifiers>> {
        GameplayModifiers::New_GameplayModifiers_EnergyType__cordl_bool__cordl_bool__cordl_bool_GameplayModifiers_EnabledObstacleType__cordl_bool__cordl_bool__cordl_bool__cordl_bool_GameplayModifiers_SongSpeed__cordl_bool__cordl_bool__cordl_bool__cordl_bool__cordl_bool1(
            mods._energyType,
            mods._noFailOn0Energy,
            mods._instaFail,
            mods._failOnSaberClash,
            mods._enabledObstacleType,
            mods._noBombs,
            false,
            mods._strictAngles,
            mods._disappearingArrows,
            mods._songSpeed,
            mods._noBombs,
            mods._ghostNotes,
            mods._proMode,
            mods._zenMode,
            mods._smallCubes,
        )
    }

    pub async fn play_song(
        &mut self,
        level: &PreviewBeatmapLevel,
        characteristic: &BeatmapCharacteristicSO,
        difficulty: BeatmapDifficulty,
        packet: &PlaySong,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(flow) = &self.flow {
            let loaded_level = self.get_level_from_preview(level).await?;
            if let Some(beatmap_level) = loaded_level {
                // Implementation for playing song would go here
                // Note: Direct Unity calls would need to be handled differently in Rust
            }
        }
        Ok(())
    }

    pub fn return_to_menu(&self) {
        // Implementation would depend on how Unity scene management is handled
    }

    pub async fn has_dlc_level(&self, level_id: &str) -> Result<bool, Box<dyn std::error::Error>> {
        if !level_id.starts_with("custom_level_") {
            info!("{}", level_id);
        }
        // Implementation would depend on DLC checking mechanism
        Ok(false)
    }

    pub async fn get_level_from_preview(
        &self,
        level: &PreviewBeatmapLevel,
    ) -> Result<Option<BeatmapLevel>, Box<dyn std::error::Error>> {
        // Implementation would depend on how beatmap levels are loaded
        Ok(None)
    }
}
