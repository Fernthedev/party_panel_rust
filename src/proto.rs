pub mod items {
    include!(concat!(env!("OUT_DIR"), "/partypanel.items.rs"));
}

pub mod packets {
    include!(concat!(env!("OUT_DIR"), "/partypanel.packets.rs"));
}
// include!(concat!(env!("OUT_DIR"), "/partypanel.items.rs"));
// include!(concat!(env!("OUT_DIR"), "/partypanel.packets.rs"));

pub enum PacketType {
    SongList = 0,
    Command = 1,
    NowPlaying = 2,
    NowPlayingUpdate = 3,
    PlaySong = 4,
    PreviewSong = 5,
    DownloadSong = 6,
    AllSongs = 7,
}
pub enum CommandType {
    Unspecified = 0,
    Heartbeat = 1,
    ReturnToMenu = 2,
}

impl From<i32> for PacketType {
    fn from(value: i32) -> Self {
        match value {
            0 => PacketType::SongList,
            1 => PacketType::Command,
            2 => PacketType::NowPlaying,
            3 => PacketType::NowPlayingUpdate,
            4 => PacketType::PlaySong,
            5 => PacketType::PreviewSong,
            6 => PacketType::DownloadSong,
            7 => PacketType::AllSongs,
            _ => panic!("Invalid packet type"),
        }
    }
}

impl From<i32> for CommandType {
    fn from(value: i32) -> Self {
        match value {
            0 => CommandType::Unspecified,
            1 => CommandType::Heartbeat,
            2 => CommandType::ReturnToMenu,
            _ => panic!("Invalid command type"),
        }
    }
}

pub trait PartyPacket: prost::Message {
    fn get_type(&self) -> PacketType;
}

impl PartyPacket for packets::SongList {
    fn get_type(&self) -> PacketType {
        PacketType::SongList
    }
}
impl PartyPacket for packets::Command {
    fn get_type(&self) -> PacketType {
        PacketType::Command
    }
}
impl PartyPacket for packets::NowPlaying {
    fn get_type(&self) -> PacketType {
        PacketType::NowPlaying
    }
}
impl PartyPacket for packets::NowPlayingUpdate {
    fn get_type(&self) -> PacketType {
        PacketType::NowPlayingUpdate
    }
}
impl PartyPacket for packets::PlaySong {
    fn get_type(&self) -> PacketType {
        PacketType::PlaySong
    }
}
impl PartyPacket for packets::PreviewSong {
    fn get_type(&self) -> PacketType {
        PacketType::PreviewSong
    }
}
impl PartyPacket for packets::DownloadSong {
    fn get_type(&self) -> PacketType {
        PacketType::DownloadSong
    }
}
impl PartyPacket for packets::AllSongs {
    fn get_type(&self) -> PacketType {
        PacketType::AllSongs
    }
}
