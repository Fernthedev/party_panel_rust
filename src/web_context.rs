use std::collections::HashMap;

use bs_cordl::{
    GlobalNamespace::{
        BeatmapCharacteristicSO, BeatmapDifficulty, BeatmapKey, BeatmapLevel, GameplayModifiers,
        GameplayModifiers_EnabledObstacleType, GameplayModifiers_EnergyType,
        GameplayModifiers_SongSpeed, MainFlowCoordinator, MenuTransitionsHelper, PracticeSettings,
        RecordingToolManager_SetupData, SoloFreePlayFlowCoordinator,
    },
    System::{Nullable_1, Threading::CancellationTokenSource},
    UnityEngine::Resources,
    HMUI::NoTransitionsButton,
};
use bytes::{Buf, Bytes, BytesMut};
use futures::TryStreamExt;
use prost::Message;
use quest_hook::libil2cpp::{Gc, Il2CppString};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::info;

use crate::{
    party_panel_run_on_main_thread,
    proto::{
        self,
        items::{
            gameplay_modifiers::{EnabledObstacleType, EnergyType, SongSpeed},
            PreviewBeatmapLevel,
        },
        packets::{
            AllSongs, Command, DownloadSong, NowPlaying, NowPlayingUpdate, PlaySong, PreviewSong,
            SongList,
        },
        CommandType, PacketType, PartyPacket,
    },
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
        mods: &proto::items::GameplayModifiers,
    ) -> quest_hook::libil2cpp::Result<Gc<GameplayModifiers>> {
        GameplayModifiers::New_GameplayModifiers_EnergyType__cordl_bool__cordl_bool__cordl_bool_GameplayModifiers_EnabledObstacleType__cordl_bool__cordl_bool__cordl_bool__cordl_bool_GameplayModifiers_SongSpeed__cordl_bool__cordl_bool__cordl_bool__cordl_bool__cordl_bool1(
            energy_type_from_i32(mods.energy_type),
            mods.no_fail_on_0_energy,
            mods.insta_fail,
            mods.fail_on_saber_clash,
            obstacle_type_from_i32(mods.enabled_obstacle_type),
            mods.no_bombs,
            false,
            mods.strict_angles,
            mods.disappearing_arrows,
            song_speed_from_i32(mods.song_speed),
            mods.no_bombs,
            mods.ghost_notes,
            mods.pro_mode,
            mods.zen_mode,
            mods.small_cubes,
        )
    }

    pub async fn play_song(
        &mut self,
        level: &PreviewBeatmapLevel,
        characteristic: Gc<BeatmapCharacteristicSO>,
        difficulty: BeatmapDifficulty,
        packet: &PlaySong,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.flow = Resources::FindObjectsOfTypeAll_1::<Gc<MainFlowCoordinator>>()?
            .as_slice()
            .first()
            .map(|flow| flow._soloFreePlayFlowCoordinator);

        let Some(flow) = &self.flow else {
            return Ok(());
        };

        extern "C" fn click_solo_button(_: *mut std::ffi::c_void) {
            let mut solo_button = Resources::FindObjectsOfTypeAll_1::<Gc<NoTransitionsButton>>()
                .unwrap()
                .as_slice()
                .iter()
                .cloned()
                .find(|x| {
                    !x.is_null()
                        && x
                            .clone()
                            .get_gameObject()
                            .unwrap()
                            .get_name()
                            .unwrap()
                            .to_string_lossy()
                            == "SoloButton"
                })
                .expect("No solo button found");
            solo_button.get_onClick().unwrap().Invoke().unwrap();
        }

        unsafe { party_panel_run_on_main_thread(click_solo_button, std::ptr::null_mut()) }

        let loaded_level = self.get_level_from_preview(level).await?;
        let Some(beatmap_level) = loaded_level else {
            return Ok(());
        };


        let mut menu_scene_setup_data =
            Resources::FindObjectsOfTypeAll_1::<Gc<MenuTransitionsHelper>>()?
                .as_slice()
                .first()
                .copied()
                .ok_or("No MenuTransitionsHelper found")?;

        let key = BeatmapKey {
            beatmapCharacteristic: characteristic,
            difficulty,
            levelId: beatmap_level.levelID,
        };
        let mut gameplay_setup_view_controller = flow._gameplaySetupViewController;
        let environment_settings =
            gameplay_setup_view_controller.get_environmentOverrideSettings()?;
        let scheme = gameplay_setup_view_controller
            .get_colorSchemesSettings()?
            .GetSelectedColorScheme()?;
        let settings = gameplay_setup_view_controller.get_playerSettings()?;

        let modifiers = Self::convert_modifiers(packet.gameplay_modifiers.as_ref().unwrap())?;

        menu_scene_setup_data.StartStandardLevel_OverrideEnvironmentSettings_ColorScheme__cordl_bool_ColorScheme_GameplayModifiers_PlayerSpecificSettings_PracticeSettings_EnvironmentsListModel_Il2CppString__cordl_bool_Action_Action_1_Action_2_Nullable_1_0(
            Il2CppString::new("Solo"),
            key,
            beatmap_level,
            environment_settings,
            scheme,
            false,
            Gc::null(),
            modifiers,
            settings,
            Gc::null(),
            Gc::null(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Nullable_1::default() ,
        )?;

        // Action<IBeatmapLevel> SongLoaded = (loadedLevel) =>
        // {
        //     MenuTransitionsHelper _menuSceneSetupData = Resources.FindObjectsOfTypeAll<MenuTransitionsHelper>().First();
        //     IDifficultyBeatmap diffbeatmap = loadedLevel.beatmapLevelData.GetDifficultyBeatmap(characteristic, difficulty);
        //     GameplaySetupViewController gameplaySetupViewController = (GameplaySetupViewController)typeof(SinglePlayerLevelSelectionFlowCoordinator).GetField("_gameplaySetupViewController", BindingFlags.NonPublic | BindingFlags.Instance).GetValue(flow);
        //     OverrideEnvironmentSettings environmentSettings = gameplaySetupViewController.environmentOverrideSettings;
        //     ColorScheme scheme = gameplaySetupViewController.colorSchemesSettings.GetSelectedColorScheme();
        //     PlayerSpecificSettings settings = gameplaySetupViewController.playerSettings;
        //     //TODO: re add modifier customizability

        //     GameplayModifiers modifiers = ConvertModifiers(packet.gameplayModifiers);
        //     _menuSceneSetupData.StartStandardLevel(
        //         "Solo",
        //         diffbeatmap,
        //         diffbeatmap.level,
        //         environmentSettings,
        //         scheme,
        //         modifiers,
        //         settings,
        //         null,
        //         "Menu",
        //         false,
        //         false,
        //         null,
        //         new Action<StandardLevelScenesTransitionSetupDataSO, LevelCompletionResults>((StandardLevelScenesTransitionSetupDataSO q, LevelCompletionResults r) => { }),
        //         new Action<LevelScenesTransitionSetupDataSO, LevelCompletionResults>((LevelScenesTransitionSetupDataSO q, LevelCompletionResults r) => { })
        //     );
        // };
        // HMMainThreadDispatcher.instance.Enqueue(() =>
        // {
        //     NoTransitionsButton button = Resources.FindObjectsOfTypeAll<NoTransitionsButton>().Where(x => x != null && x.gameObject.name == "SoloButton").FirstOrDefault();
        //     button.onClick.Invoke();
        // });
        // if (true)
        // {
        //     var result = await GetLevelFromPreview(level);
        //     if ( !(result?.isError == true))
        //     {
        //         SongLoaded(result?.beatmapLevel);
        //         return;
        //     }
        // }
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
    ) -> Result<Option<Gc<BeatmapLevel>>, Box<dyn std::error::Error>> {
        // Implementation would depend on how beatmap levels are loaded
        Ok(None)
    }
}

fn energy_type_from_i32(value: i32) -> GameplayModifiers_EnergyType {
    match value {
        0 => GameplayModifiers_EnergyType::Bar,
        1 => GameplayModifiers_EnergyType::Battery,
        _ => GameplayModifiers_EnergyType::Bar,
    }
}

fn obstacle_type_from_i32(value: i32) -> GameplayModifiers_EnabledObstacleType {
    match value {
        0 => GameplayModifiers_EnabledObstacleType::All,
        1 => GameplayModifiers_EnabledObstacleType::FullHeightOnly,
        2 => GameplayModifiers_EnabledObstacleType::NoObstacles,
        _ => GameplayModifiers_EnabledObstacleType::All,
    }
}

fn song_speed_from_i32(value: i32) -> GameplayModifiers_SongSpeed {
    match value {
        0 => GameplayModifiers_SongSpeed::Normal,
        1 => GameplayModifiers_SongSpeed::Faster,
        2 => GameplayModifiers_SongSpeed::Slower,
        3 => GameplayModifiers_SongSpeed::SuperFast,
        _ => GameplayModifiers_SongSpeed::Normal,
    }
}
