#ifdef ANDROID
#include "songcore/shared/SongCore.hpp"
#include "bsml/shared/BSML/MainThreadScheduler.hpp"
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
extern "C" void party_panel_run_on_main_thread(void (*func)(void *),
                                               void *arg) {
  BSML::MainThreadScheduler::Schedule([=]() { func(arg); });
}

#endif