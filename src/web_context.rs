use anyhow::{anyhow, Context};
use bs_cordl::{
    GlobalNamespace::{
        AdditionalContentModel, BeatmapCharacteristicSO, BeatmapDifficulty, BeatmapKey,
        BeatmapLevel, BeatmapLevelsModel, EntitlementStatus, GameplayModifiers,
        GameplayModifiers_EnabledObstacleType, GameplayModifiers_EnergyType,
        GameplayModifiers_SongSpeed, MainFlowCoordinator, MenuTransitionsHelper, PlayerData,
        PlayerDataModel, PracticeSettings, SoloFreePlayFlowCoordinator,
        StandardLevelReturnToMenuController,
    },
    System::{
        self, Nullable_1,
        Threading::{CancellationToken, CancellationTokenSource},
    },
    UnityEngine::Resources,
    HMUI::NoTransitionsButton,
};
use bytes::{Buf, Bytes, BytesMut};
use futures::{
    future::{self},
    TryStreamExt,
};
use itertools::Itertools;
use prost::Message;
use quest_hook::libil2cpp::{Gc, Il2CppString};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tracing::info;

use crate::{
    async_utils::Il2CPPFutureAwaitable,
    party_panel_run_on_main_thread,
    proto::{
        self,
        items::PreviewBeatmapLevel,
        packets::{Command, DownloadSong, PlaySong, SongList},
        CommandType, PacketType, PartyPacket,
    },
};

pub struct WebContext {
    pub songs: Vec<SongData>,
    pub player_data: Gc<PlayerDataModel>,
    pub level_cancellation_token_source: Option<Gc<CancellationTokenSource>>,
    pub get_status_cancellation_token_source: Option<Gc<CancellationTokenSource>>,
    pub flow: Option<Gc<SoloFreePlayFlowCoordinator>>,
    pub socket: TcpStream, //WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
}

pub struct SongData {
    pub hash: SongId,
    pub level: Gc<BeatmapLevel>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SongId(pub String);

impl WebContext {
    pub async fn read_loop(&mut self) -> anyhow::Result<()> {
        let mut buf: BytesMut = BytesMut::with_capacity(1024);

        loop {
            // buffer the size of UTF8 "moon"
            let mut header = [0u8; 4];
            self.socket.read_exact(&mut header).await?;

            // continue? restart loop
            if header != "moon".as_bytes() {
                return Err(anyhow!("Invalid header"));
            }

            let packet_type: PacketType = self.socket.read_i32().await?.into();

            let len = self.socket.read_u64().await? as usize;
            // resize to read the amount we need
            buf.resize(len, 0);

            let buffer = self.socket.read_exact(&mut buf).await?;
            if buffer < len {
                return Err(anyhow!("Failed to read the full buffer"));
            }

            let error = self.parse_packet(packet_type, buf.copy_to_bytes(len)).await;
            if let Err(e) = error {
                info!("Error parsing packet: {:?}", e);
            }
        }

        Ok(())
    }

    pub async fn update(&mut self) -> anyhow::Result<()> {
        let player_data = self.player_data.clone();

        if let Some(mut source) = self.get_status_cancellation_token_source {
            source.Cancel_0()?;
        }
        self.get_status_cancellation_token_source = Some(CancellationTokenSource::New_0()?);

        let token = self
            .get_status_cancellation_token_source
            .unwrap()
            .get_Token()?;

        let mut level_futures = Vec::with_capacity(self.songs.len());
        let levels = self.songs.iter().map(|s| s.level.clone()).collect_vec();
        for level in levels {
            let preview_level =
                Self::convert_to_packet_type(level, player_data._playerData, Some(token.clone()));
            level_futures.push(preview_level);
        }

        let levels = future::try_join_all(level_futures).await?;

        self.write_packet(SongList { levels }).await?;

        Ok(())
    }

    ///
    ///
    ///
    async fn parse_packet(&mut self, packet_type: PacketType, data: Bytes) -> anyhow::Result<()> {
        /*
                   if (packet.Type == PacketType.PlaySong)
           {
               PlaySong playSong = packet.SpecificPacket as PlaySong;

               var desiredLevel = Plugin.masterLevelList.First(x => x.levelID == playSong.levelId);
               var desiredCharacteristic = desiredLevel.previewDifficultyBeatmapSets.Select(level => level.beatmapCharacteristic).First(x => x.serializedName == playSong.characteristic.Name);
               BeatmapDifficulty desiredDifficulty;
               playSong.difficulty.BeatmapDifficultyFromSerializedName(out desiredDifficulty);

               SaberUtilities.PlaySong(desiredLevel, desiredCharacteristic, desiredDifficulty, playSong);
           }
           else if (packet.Type == PacketType.Command)
           {
               Command command = packet.SpecificPacket as Command;
               if (command.commandType == Command.CommandType.ReturnToMenu)
               {
                   SaberUtilities.ReturnToMenu();
               }
           }
           else if (packet.Type == PacketType.DownloadSong)
           {
               DownloadSong download = packet.SpecificPacket as DownloadSong;

               Task.Run(async () => { await BeatSaverDownloader.Misc.SongDownloader.Instance.DownloadSong(Plugin.Client.Beatmap(download.songKey).Result, CancellationToken.None); SongCore.Loader.Instance.RefreshSongs(); });
           }
        */

        match packet_type {
            PacketType::PlaySong => {
                let playsong = PlaySong::decode(data)?;
                let desired_level = self
                    .songs
                    .iter()
                    .find(|x| x.hash.0 == playsong.level_id)
                    .ok_or_else(|| anyhow!("Level not found"))?;

                let desired_characteristic = self
                    .player_data
                    ._playerDataFileModel
                    ._beatmapCharacteristicCollection
                    .GetBeatmapCharacteristicBySerializedName(Il2CppString::new(
                        &playsong.characteristic.as_ref().unwrap().name,
                    ))
                    .context(anyhow!("Characteristic not found"))?;

                let desired_diff = difficulty_from_name(&playsong.difficulty);
                self.play_song(
                    desired_level.level,
                    desired_characteristic,
                    desired_diff,
                    &playsong,
                )
                .await?;
            }
            PacketType::Command => {
                let command = Command::decode(data)?;
                let command_type = CommandType::from(command.command_type);
                if let CommandType::ReturnToMenu = command_type {
                    self.return_to_menu();
                    // return to menu
                }
            }
            PacketType::DownloadSong => {
                let download_song = DownloadSong::decode(data)?;
                // TODO: download song
            }
            _ => {}
        }

        Ok(())
    }

    pub async fn write_packet(&mut self, packet: impl PartyPacket) -> anyhow::Result<()> {
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
        beatmap_level: Gc<BeatmapLevel>,
        characteristic: Gc<BeatmapCharacteristicSO>,
        difficulty: BeatmapDifficulty,
        packet: &PlaySong,
    ) -> anyhow::Result<()> {
        self.flow = Resources::FindObjectsOfTypeAll_1::<Gc<MainFlowCoordinator>>()?
            .as_slice()
            .first()
            .map(|flow| flow._soloFreePlayFlowCoordinator);

        let Some(flow) = self.flow else {
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
                        && x.clone()
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

        let mut menu_scene_setup_data =
            Resources::FindObjectsOfTypeAll_1::<Gc<MenuTransitionsHelper>>()?
                .as_slice()
                .first()
                .copied()
                .ok_or(anyhow!("No MenuTransitionsHelper found"))?;

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
        extern "C" fn return_to_main_menu_callback(_: *mut std::ffi::c_void) {
            let Ok(controllers) =
                Resources::FindObjectsOfTypeAll_1::<Gc<StandardLevelReturnToMenuController>>()
            else {
                return;
            };
            let Some(mut controller) = controllers.as_slice().first().cloned() else {
                return;
            };
            let _ = controller.ReturnToMenu();
        }

        unsafe {
            party_panel_run_on_main_thread(return_to_main_menu_callback, std::ptr::null_mut());
        }
    }

    // public static async Task<bool> HasDLCLevel(string levelId, AdditionalContentModel additionalContentModel = null)
    // {
    //     if(!levelId.StartsWith("custom_level_"))
    //     {
    //         Logger.Info(levelId);
    //     }
    //     additionalContentModel = additionalContentModel ?? Resources.FindObjectsOfTypeAll<AdditionalContentModel>().FirstOrDefault();

    //     if (additionalContentModel != null)
    //     {
    //         getStatusCancellationTokenSource?.Cancel();
    //         getStatusCancellationTokenSource = new CancellationTokenSource();

    //         var token = getStatusCancellationTokenSource.Token;
    //         return await additionalContentModel.GetLevelEntitlementStatusAsync(levelId, token) == AdditionalContentModel.EntitlementStatus.Owned;
    //     }

    //     return false;
    // }
    pub async fn has_dlc_level(
        level_id: &str,
        additional_content_model: Option<Gc<AdditionalContentModel>>,
        token: Option<CancellationToken>,
    ) -> anyhow::Result<bool> {
        if !level_id.starts_with("custom_level_") {
            info!("{}", level_id);
        }

        let model = additional_content_model.or_else(|| {
            Resources::FindObjectsOfTypeAll_1::<Gc<AdditionalContentModel>>()
                .ok()
                .and_then(|models| models.as_slice().first().cloned())
        });

        let Some(mut model) = model else {
            return Ok(false);
        };

        let status = model
            .IAdditionalContentEntitlementModel_GetLevelEntitlementStatusAsync(
                Il2CppString::new(level_id),
                token.unwrap_or_default(),
            )?
            .into_awaitable()
            .await?;

        Ok(status == EntitlementStatus::Owned)
    }

    // public static async Task<BeatmapLevelsModel.GetBeatmapLevelResult?> GetLevelFromPreview(IPreviewBeatmapLevel level, BeatmapLevelsModel beatmapLevelsModel = null)
    // {
    //     beatmapLevelsModel = beatmapLevelsModel ?? Resources.FindObjectsOfTypeAll<BeatmapLevelsModel>().FirstOrDefault();

    //     if (beatmapLevelsModel != null)
    //     {
    //         getLevelCancellationTokenSource?.Cancel();
    //         getLevelCancellationTokenSource = new CancellationTokenSource();

    //         var token = getLevelCancellationTokenSource.Token;

    //         BeatmapLevelsModel.GetBeatmapLevelResult? result = null;
    //         try
    //         {
    //             result = await beatmapLevelsModel.GetBeatmapLevelAsync(level.levelID, token);
    //         }
    //         catch (OperationCanceledException) { }
    //         if (result?.isError == true || result?.beatmapLevel == null)
    //         {
    //             Logger.Error("Failed to load Level");
    //             return null; //Null out entirely in case of error
    //         }
    //         return result;
    //     }
    //     return null;
    // }
    pub fn get_level_from_preview(
        &mut self,
        level: &PreviewBeatmapLevel,
    ) -> anyhow::Result<Option<Gc<BeatmapLevel>>> {
        let beatmap_levels_model = Resources::FindObjectsOfTypeAll_1::<Gc<BeatmapLevelsModel>>()?
            .as_slice()
            .first()
            .cloned();

        let Some(mut beatmap_levels_model) = beatmap_levels_model else {
            return Ok(None);
        };

        let result = beatmap_levels_model.GetBeatmapLevel(Il2CppString::new(&level.level_id));

        match result {
            Ok(level) => Ok(Some(level)),
            _ => {
                info!("Failed to load Level");
                Ok(None)
            }
        }
    }

    //     public static async Task<PreviewBeatmapLevel> ConvertToPacketType(IPreviewBeatmapLevel x, PlayerData playerData)
    // {
    //     //Make packet level
    //     var level = new PreviewBeatmapLevel();
    //     try
    //     {
    //         //Set Parameters;
    //         level.LevelId = x.levelID;
    //         level.Name = x.songName;
    //         level.SubName = x.songSubName;
    //         level.Author = x.songAuthorName;
    //         level.Mapper = x.levelAuthorName;
    //         level.BPM = x.beatsPerMinute;
    //         level.Duration = TimeExtensions.MinSecDurationText(x.songDuration);
    //         level.Favorited = playerData.favoritesLevelIds.Contains(x.levelID);
    //         level.Owned = await SaberUtilities.HasDLCLevel(x.levelID);
    //         if(!level.Owned)
    //         {
    //             level.OwnedJustificaton = "Unowned DLC Level";
    //         }
    //         if(x is CustomPreviewBeatmapLevel)
    //         {
    //             var extras = Collections.RetrieveExtraSongData(new string(x.levelID.Skip(13).ToArray()));
    //             var requirements = extras?._difficulties.SelectMany((x) => { return x.additionalDifficultyData._requirements; });
    //             List<string> missingReqs = new List<string>();
    //             if (
    //                 (requirements?.Count() > 0) &&
    //                 (!requirements?.ToList().All(x => {
    //                     if(Collections.capabilities.Contains(x))
    //                     {
    //                         return true;
    //                     }
    //                     else
    //                     {
    //                         missingReqs.Add(x);
    //                         return false;
    //                     }
    //                 }) ?? false)
    //             )
    //             {
    //                 level.Owned = false;
    //                 level.OwnedJustificaton = "Missing " + missingReqs.Aggregate((x, x2) => { return x + x2; });
    //             }
    //         }

    //         OverrideLabels labels = new OverrideLabels();
    //         var songData = Collections.RetrieveExtraSongData(new string(x.levelID.Skip(13).ToArray()));

    //         Dictionary<string, OverrideLabels> LevelLabels = new Dictionary<string, OverrideLabels>();
    //         LevelLabels.Clear();
    //         if (songData != null)
    //         {
    //             foreach (SongCore.Data.ExtraSongData.DifficultyData diffLevel in songData._difficulties)
    //             {

    //                 var difficulty = diffLevel._difficulty;
    //                 string characteristic = diffLevel._beatmapCharacteristicName;

    //                 if (!LevelLabels.ContainsKey(characteristic))
    //                 {
    //                     LevelLabels.Add(characteristic, new OverrideLabels());
    //                 }

    //                 var charLabels = LevelLabels[characteristic];
    //                 if (!string.IsNullOrWhiteSpace(diffLevel._difficultyLabel))
    //                 {

    //                     switch (difficulty)
    //                     {
    //                         case BeatmapDifficulty.Easy:
    //                             charLabels.EasyOverride = diffLevel._difficultyLabel;
    //                             break;
    //                         case BeatmapDifficulty.Normal:
    //                             charLabels.NormalOverride = diffLevel._difficultyLabel;
    //                             break;
    //                         case BeatmapDifficulty.Hard:
    //                             charLabels.HardOverride = diffLevel._difficultyLabel;
    //                             break;
    //                         case BeatmapDifficulty.Expert:
    //                             charLabels.ExpertOverride = diffLevel._difficultyLabel;
    //                             break;
    //                         case BeatmapDifficulty.ExpertPlus:
    //                             charLabels.ExpertPlusOverride = diffLevel._difficultyLabel;
    //                             break;
    //                     }
    //                 }
    //             }
    //         }
    //         level.chars = x.previewDifficultyBeatmapSets.Select((PreviewDifficultyBeatmapSet set) => { Characteristic Char = new Characteristic(); Char.Name = set.beatmapCharacteristic.serializedName;    Char.diffs = set.beatmapDifficulties.Select((BeatmapDifficulty diff)=> { return Name(LevelLabels.ContainsKey(Char.Name) ? LevelLabels[Char.Name]: null, diff); }).ToArray(); return Char; }).ToArray();
    //         if (x.GetType().Name.Contains("BeatmapLevelSO"))
    //         {
    //             Texture2D tex;
    //             Sprite sprite = (await x.GetCoverImageAsync(System.Threading.CancellationToken.None));
    //             try
    //             {
    //                 tex = sprite.texture;
    //             }
    //             catch
    //             {
    //                 tex = GetFromUnreadable((x as CustomPreviewBeatmapLevel)?.defaultCoverImage.texture, sprite.textureRect);
    //             }
    //             if (!(x is CustomPreviewBeatmapLevel) || tex == null || !tex.isReadable)
    //             {
    //                 tex = GetFromUnreadable(tex, InvertAtlas(sprite.textureRect));
    //             }
    //             level.cover = tex.EncodeToJPG();
    //         }
    //         else
    //         {
    //             if(x is CustomPreviewBeatmapLevel)
    //             {
    //                 string path = Path.Combine(((CustomPreviewBeatmapLevel)x).customLevelPath, ((CustomPreviewBeatmapLevel)x).standardLevelInfoSaveData.coverImageFilename);
    //                 if (File.Exists(path))
    //                 {
    //                     level.coverPath = path;
    //                 }
    //             }
    //         }
    //     }
    //     catch(Exception e)
    //     {
    //         Logger.Error(e.ToString());
    //     }
    //     return level;
    // }
    pub async fn convert_to_packet_type(
        mut x: Gc<BeatmapLevel>,
        mut player_data: Gc<PlayerData>,
        token: Option<CancellationToken>,
    ) -> anyhow::Result<PreviewBeatmapLevel> {
        fn format_duration(duration: f32) -> String {
            if duration.is_nan() {
                return String::new();
            }
            let minutes = (duration / 60.0) as i32;
            let seconds = (duration % 60.0) as i32;
            format!("{}:{:02}", minutes, seconds)
        }

        let mut level = PreviewBeatmapLevel {
            level_id: x.levelID.to_string_lossy(),
            name: x.songName.to_string_lossy().to_string(),
            sub_name: x.songSubName.to_string_lossy().to_string(),
            author: x.songAuthorName.to_string_lossy().to_string(),
            mapper: x
                .allMappers
                .as_slice()
                .iter()
                .map(|m| m.to_string_lossy())
                .join(","),
            bpm: x.beatsPerMinute,
            duration: format_duration(x.songDuration),
            favorited: player_data.get_favoritesLevelIds()?.Contains(x.levelID)?,
            ..Default::default()
        };
        level.owned = Self::has_dlc_level(&level.level_id, None, token).await?;

        if !level.owned {
            level.owned_justification = "Unowned DLC Level".to_string();
        }
        level.chars = System::Linq::Enumerable::ToList(x.GetBeatmapKeys()?)?
            ._items
            .as_slice()
            .iter()
            .filter(|i| **i != BeatmapKey::default())
            .map(|i| (i.beatmapCharacteristic, i.difficulty))
            .chunk_by(|(characteristic, _)| *characteristic)
            .into_iter()
            .map(
                |(characteristic, difficulties)| -> quest_hook::libil2cpp::Result<_> {
                    let char = proto::items::Characteristic {
                        name: characteristic
                            .clone()
                            .get_serializedName()?
                            .to_string_lossy()
                            .to_string(),
                        diffs: difficulties
                            .map(|(_, diff)| difficulty_name(diff))
                            .collect::<Vec<_>>(),
                    };
                    Ok(char)
                },
            )
            .try_collect()?;

        // Skip cover image handling for now as it requires more complex Unity texture manipulation

        Ok(level)
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

fn difficulty_name(difficulty: BeatmapDifficulty) -> String {
    match difficulty {
        BeatmapDifficulty::Easy => "Easy".to_string(),
        BeatmapDifficulty::Normal => "Normal".to_string(),
        BeatmapDifficulty::Hard => "Hard".to_string(),
        BeatmapDifficulty::Expert => "Expert".to_string(),
        BeatmapDifficulty::ExpertPlus => "ExpertPlus".to_string(),
        _ => "Unknown".to_string(),
    }
}

fn difficulty_from_name(name: &str) -> BeatmapDifficulty {
    match name {
        "Easy" => BeatmapDifficulty::Easy,
        "Normal" => BeatmapDifficulty::Normal,
        "Hard" => BeatmapDifficulty::Hard,
        "Expert" => BeatmapDifficulty::Expert,
        "ExpertPlus" => BeatmapDifficulty::ExpertPlus,
        _ => BeatmapDifficulty::default(),
    }
}
