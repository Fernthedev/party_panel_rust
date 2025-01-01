#![feature(box_patterns, extend_one)]
#![feature(generic_arg_infer)]

use std::ffi::{c_char, CString};

use bs_cordl::GlobalNamespace::{
    AudioClipAsyncLoader, BeatmapData, BeatmapDataLoader, BeatmapKey, BeatmapLevel,
    BeatmapLevelsEntitlementModel, BeatmapLevelsModel, ColorScheme, EnvironmentsListModel,
    GameplayModifiers, IReadonlyBeatmapData, LevelCompletionResults, NoteData,
    OverrideEnvironmentSettings, PlayerSpecificSettings, PracticeSettings,
    RecordingToolManager_SetupData, SettingsManager, StandardLevelScenesTransitionSetupDataSO,
};
use bs_cordl::TMPro::TextMeshPro;
use bs_cordl::UnityEngine::{self};
use quest_hook::hook;
use quest_hook::libil2cpp::{Gc, Il2CppString};
use scotland2_rs::scotland2_raw::CModInfo;
use scotland2_rs::ModInfoBuf;

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
}

#[hook("", "StandardLevelScenesTransitionSetupDataSO", "Finsih")]
fn StandardLevelScenesTransitionSetupDataSO_Finish(
    this: &mut StandardLevelScenesTransitionSetupDataSO,
    level_completion_results: Gc<LevelCompletionResults>,
) {
    StandardLevelScenesTransitionSetupDataSO_Finish.original(this, level_completion_results);
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
extern "C" fn late_load() {
    StandardLevelScenesTransitionSetupDataSO_Init
        .install()
        .unwrap();
    StandardLevelScenesTransitionSetupDataSO_Finish
        .install()
        .unwrap();
}
