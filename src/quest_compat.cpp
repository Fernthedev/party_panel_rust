#ifdef ANDROID
#include "songcore/shared/SongCore.hpp"
#endif

namespace GlobalNamespace {
struct BeatmapLevelPack;
}

extern "C" void
party_panel_on_song_load(GlobalNamespace::BeatmapLevelPack *const *const array,
                         size_t len);

#ifdef ANDROID
extern "C" void quest_compat_init() {
  SongCore::API::Loading::GetCustomLevelPacksRefreshedEvent() +=
      [](SongCore::SongLoader::CustomBeatmapLevelsRepository *repository) {
        auto levels = repository->GetBeatmapLevelPacks();
        party_panel_on_song_load(levels.data(), levels.size());
      };
}
#endif