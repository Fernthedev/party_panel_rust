#![feature(box_patterns, extend_one)]
#![feature(generic_arg_infer)]
#![feature(lock_value_accessors)]

use std::ffi::CStr;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use anyhow::Context;
use bs_cordl::GlobalNamespace::{
    AudioClipAsyncLoader, BeatmapDataLoader, BeatmapKey, BeatmapLevel, BeatmapLevelPack,
    BeatmapLevelsEntitlementModel, BeatmapLevelsModel, ColorScheme, EnvironmentsListModel,
    GameplayModifiers, LevelCompletionResults, OverrideEnvironmentSettings, PlayerDataModel,
    PlayerSpecificSettings, PracticeSettings, RecordingToolManager_SetupData, ScoreController,
    SettingsManager, StandardLevelScenesTransitionSetupDataSO,
};
use bs_cordl::UnityEngine::Resources;
use config::Config;
use futures::StreamExt;
use proto::packets::NowPlayingUpdate;
use quest_hook::hook;
use quest_hook::libil2cpp::{Gc, Il2CppString};
use scotland2_rs::scotland2_raw::CModInfo;
use scotland2_rs::ModInfoBuf;
use tokio::net::TcpSocket;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;
use tracing::debug;
use web_context::SongData;

mod web_context;

mod async_utils;
mod config;
mod proto;

// Define a static runtime
// We don't use tokio primitives here
static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all() // Enable features like timers and I/O
        .worker_threads(1) // Single-threaded
        .build()
        .expect("Failed to create runtime")
});

static mut HEARTBEAT_HANDLE: Mutex<Option<tokio::task::JoinHandle<()>>> = Mutex::new(None);

static mut WEB_CONTEXT: RwLock<Option<web_context::WebContext>> = RwLock::const_new(None);

async fn heartbeat_timer(mut score: Gc<ScoreController>) -> anyhow::Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        // Assuming we have similar data structures in Rust
        // This is a placeholder implementation - you'll need to adapt it
        // to your actual data structures
        let mut guard = unsafe { WEB_CONTEXT.write().await };
        let Some(context) = guard.as_mut() else {
            return Ok(());
        };

        let packet = NowPlayingUpdate {
            score: score._modifiedScore,
            accuracy: 0.0,
            elapsed: score._audioTimeSyncController._songTime as i32,
            total_time: score._audioTimeSyncController.get_songLength()? as i32,
        };

        context.write_packet(packet).await?;
    }
}

// You might want to start the timer in your setup_client function:
// tokio::spawn(async {
//     let mut interval = tokio::time::interval(Duration::from_secs(1));
//     loop {
//         interval.tick().await;
//         heartbeat_timer_elapsed().await;
//     }
// });

#[hook("", "StandardLevelScenesTransitionSetupDataSO", "Init")]
fn StandardLevelScenesTransitionSetupDataSO_Init(
    this: &mut StandardLevelScenesTransitionSetupDataSO,
    game_mode: Gc<Il2CppString>,
    beatmap_key: BeatmapKey,
    beatmap_level: Gc<BeatmapLevel>,
    override_environment_settings: Gc<OverrideEnvironmentSettings>,
    player_override_color_scheme: Gc<ColorScheme>,
    player_override_lightshow_colors: bool,
    beatmap_override_color_scheme: Gc<ColorScheme>,
    gameplay_modifiers: Gc<GameplayModifiers>,
    player_specific_settings: Gc<PlayerSpecificSettings>,
    practice_settings: Gc<PracticeSettings>,
    environments_list_model: Gc<EnvironmentsListModel>,
    audio_clip_async_loader: Gc<AudioClipAsyncLoader>,
    beatmap_data_loader: Gc<BeatmapDataLoader>,
    settings_manager: Gc<SettingsManager>,
    back_button_text: Gc<Il2CppString>,
    beatmap_levels_model: Gc<BeatmapLevelsModel>, // optional
    beatmap_levels_entitlement_model: Gc<BeatmapLevelsEntitlementModel>, // optional
    use_test_note_cut_sound_effects: bool,
    start_paused: bool,
    recording_tool_data: RecordingToolManager_SetupData, // optional
) {
    StandardLevelScenesTransitionSetupDataSO_Init.original(
        this,
        game_mode,
        beatmap_key,
        beatmap_level,
        override_environment_settings,
        player_override_color_scheme,
        player_override_lightshow_colors,
        beatmap_override_color_scheme,
        gameplay_modifiers,
        player_specific_settings,
        practice_settings,
        environments_list_model,
        audio_clip_async_loader,
        beatmap_data_loader,
        settings_manager,
        back_button_text,
        beatmap_levels_model,
        beatmap_levels_entitlement_model,
        use_test_note_cut_sound_effects,
        start_paused,
        recording_tool_data,
    );

    let score_controller = Resources::FindObjectsOfTypeAll_1::<Gc<ScoreController>>()
        .unwrap()
        .as_slice()
        .first()
        .copied()
        .unwrap();

    let handle = RUNTIME.spawn(async move {
        heartbeat_timer(score_controller).await.unwrap();
    });

    unsafe {
        HEARTBEAT_HANDLE.replace(Some(handle)).unwrap();
    }
}

#[hook("", "StandardLevelScenesTransitionSetupDataSO", "Finish")]
fn StandardLevelScenesTransitionSetupDataSO_Finish(
    this: &mut StandardLevelScenesTransitionSetupDataSO,
    level_completion_results: Gc<LevelCompletionResults>,
) {
    StandardLevelScenesTransitionSetupDataSO_Finish.original(this, level_completion_results);

    // consume the optional and abort the heartbeat task
    if let Some(handle) = unsafe { HEARTBEAT_HANDLE.lock().unwrap().take() } {
        handle.abort();
    }
}

#[no_mangle]
extern "C" fn setup(modinfo: *mut CModInfo) {
    unsafe {
        *modinfo = ModInfoBuf {
            // we have to let the string leak, because the CString is dropped at the end of the function
            id: ("PartyPanel").to_string(),
            version: ("1.0.0").to_string(),
            version_long: 0,
        }
        .into();
    }

    quest_hook::setup("PartyPanel");
}

#[no_mangle]
extern "C" fn party_panel_on_song_load(levels: *const *const BeatmapLevelPack, len: usize) {
    if len == 0 || levels.is_null() {
        return;
    }
    // Safety: This function assumes valid pointers and length
    unsafe {
        let levels_slice = std::slice::from_raw_parts(levels, len);

        let levels_converted = levels_slice
            .iter()
            .map(|level| Gc::from(*level))
            .flat_map(|level_pack| level_pack._beatmapLevels.as_slice().to_vec())
            .map(|level| SongData {
                // mappers love invalid UTF-8/UTF-16!
                hash: web_context::SongId(level.levelID.to_string_lossy()),
                level,
            })
            .collect::<Vec<_>>();

        RUNTIME.spawn(async {
            let mut web_context_locked = unsafe { WEB_CONTEXT.write().await };
            if let Some(web_context) = web_context_locked.as_mut() {
                web_context.songs = levels_converted;
                web_context.update().await.unwrap();
            }
        });
    }
}

extern "C" {
    fn quest_compat_init();
    pub fn party_panel_run_on_main_thread(
        func: extern "C" fn(*mut std::ffi::c_void),
        arg: *mut std::ffi::c_void,
    );
}

#[no_mangle]
extern "C" fn late_load() {
    StandardLevelScenesTransitionSetupDataSO_Init
        .install()
        .unwrap();
    StandardLevelScenesTransitionSetupDataSO_Finish
        .install()
        .unwrap();

    debug!("Setting up SongCore events");
    unsafe { quest_compat_init() };

    debug!("Setting up socket");
    let player_model = Resources::FindObjectsOfTypeAll_1::<Gc<PlayerDataModel>>()
        .expect("Failed to find PlayerDataModel 1")
        .as_slice()
        .first()
        .copied()
        .expect("Failed to find PlayerDataModel 2");

    RUNTIME.spawn(async move {
        if let Err(err) = setup_client(player_model).await {
            tracing::error!("Failed to setup client: {:?}", err);
        }
    });
}

async fn setup_client(player_model: Gc<PlayerDataModel>) -> anyhow::Result<()> {
    let id = unsafe { CStr::from_ptr(scotland2_rs::scotland2_raw::modloader_get_application_id()) };
    let path: PathBuf = format!(
        "/sdcard/ModData/{}/Configs/config.json",
        id.to_string_lossy()
    )
    .into();

    let config: Config = if tokio::fs::try_exists(&path).await.is_err() {
        let data = tokio::fs::read(path)
            .await
            .context("Config unable to be loaded")?;

        serde_json::from_slice(&data).context("Failed to parse config")?
    } else {
        let config = Config {
            addr: "127.0.0.1:8080".to_string(),
        };
        tokio::fs::write(path, serde_json::to_vec(&config)?).await?;
        config
    };

    let addr = config.addr.parse().unwrap();

    let socket = TcpSocket::new_v4()?;
    let stream = socket.connect(addr).await?;

    // let (ws_stream, _response) = connect_async(url).await.expect("Failed to connect");

    let mut web_context_locked = unsafe { WEB_CONTEXT.write().await };

    let web_context = web_context_locked.insert(web_context::WebContext {
        socket: stream,
        flow: None,
        get_status_cancellation_token_source: None,
        level_cancellation_token_source: None,
        songs: Default::default(),
        player_data: player_model,
    });
    println!("WebSocket handshake has been successfully completed");

    web_context.read_loop().await?;

    Ok(())
}
