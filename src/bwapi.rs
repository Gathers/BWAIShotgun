use crate::botsetup::BotSetup;
use crate::{Binary, Race};
use serde::de::Unexpected;
use serde::{Deserialize, Deserializer, Serialize};
use shared_memory::*;
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::mem::size_of;
use std::path::PathBuf;

#[derive(Debug, Eq, PartialEq)]
pub enum BwapiVersion {
    Bwapi375,
    Bwapi412,
    Bwapi420,
    Bwapi440,
}

impl BwapiVersion {
    pub fn from_u32(crc: u32) -> Option<BwapiVersion> {
        match crc {
            0x71CB208B => Some(Self::Bwapi440),
            0xD1E0DDDF => Some(Self::Bwapi420),
            0x267BD0D5 => Some(Self::Bwapi412),
            0x4E39C88A => Some(Self::Bwapi375),
            0x41128276 => Some(Self::Bwapi375),
            _ => None,
        }
    }

    pub fn version_short(&self) -> &'static str {
        match self {
            Self::Bwapi375 => "375",
            Self::Bwapi412 => "412",
            Self::Bwapi420 => "420",
            Self::Bwapi440 => "440",
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GameInstance {
    pub server_process_id: u32,
    pub is_connected: bool,
    pub last_keep_alive_time: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GameTable {
    pub game_instances: [GameInstance; 8],
}

pub struct GameTableAccess {
    game_table: Option<Shmem>,
}

impl GameTableAccess {
    pub fn new() -> Self {
        Self { game_table: None }
    }

    pub fn get_game_table(&mut self) -> Option<GameTable> {
        if self.game_table.is_none() {
            let shmmem = ShmemConf::new()
                .size(size_of::<GameTable>())
                .allow_raw(true)
                .os_id(r"Local\bwapi_shared_memory_game_list")
                .open();
            self.game_table = shmmem.ok();
        }
        self.game_table
            .as_ref()
            .map(|shmem| unsafe { *(shmem.as_ptr() as *const GameTable) })
    }

    pub fn all_slots_filled(&mut self) -> bool {
        self.get_game_table()
            .map(|table| {
                // eprintln!("{:#?}", table);
                !table
                    .game_instances
                    .iter()
                    .any(|it| it.server_process_id != 0 && !it.is_connected)
            })
            .unwrap_or(false)
    }

    pub fn has_free_slot(&mut self) -> bool {
        self.get_game_table()
            .map(|table| {
                table
                    .game_instances
                    .iter()
                    .any(|it| it.server_process_id != 0 && !it.is_connected)
            })
            .unwrap_or(false)
    }
}

pub enum BwapiConnectMode {
    Host {
        map: Option<String>,
        player_count: usize,
    },
    Join,
}

#[derive(clap::ArgEnum, Clone, Copy, Debug, Serialize, PartialEq)]
pub enum BwapiLanMode {
    LocalAreaNetworkUDP,
    LocalPC,
}

impl<'d> Deserialize<'d> for BwapiLanMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'d>,
    {
        match String::deserialize(deserializer)?.to_lowercase().as_str() {
            "u" | "localareanetworkudp" => Ok(BwapiLanMode::LocalAreaNetworkUDP),
            "p" | "localpc" => Ok(BwapiLanMode::LocalPC),
            x => Err(serde::de::Error::invalid_value(
                Unexpected::Str(x),
                &"One of LocalAreaNetworkUDP/LocalPC or u/p",
            )),
        }
    }
}

impl Display for BwapiLanMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                BwapiLanMode::LocalAreaNetworkUDP => "Local Area Network (UDP)",
                BwapiLanMode::LocalPC => "Local PC",
            }
        )
    }
}

pub enum AutoMenu {
    // Managed by bwheadless
    Unused,
    // Managed by BWAPI + injectory
    AutoMenu {
        name: String,
        race: Race,
        game_name: String,
        connect_mode: BwapiConnectMode,
        lan_mode: BwapiLanMode,
    },
}

impl Default for AutoMenu {
    fn default() -> Self {
        Self::Unused
    }
}

/// Although BWAPI can manage multiple bots with one BWAPI.ini, we'll be using one per bot
#[derive(Default)]
pub struct BwapiIni {
    pub ai_module: String,
    pub tm_module: Option<PathBuf>,
    // default: 0 - full throttle
    pub game_speed: i32,
    pub sound: bool,
    pub auto_menu: AutoMenu,
}

impl BwapiIni {
    pub fn from(bot_setup: &BotSetup) -> Self {
        Self {
            ai_module: match &bot_setup.bot_binary {
                Binary::Dll(x) => x.to_string_lossy().to_string(),
                Binary::Exe(_) | Binary::Jar(_) => "".to_string(),
            },
            tm_module: bot_setup.tournament_module.clone(),
            ..Default::default()
        }
    }
    pub fn write(&self, out: &mut impl Write) -> std::io::Result<()> {
        writeln!(out, "[ai]")?;
        writeln!(out, "ai = {}", self.ai_module)?;
        if let Some(tm) = &self.tm_module {
            writeln!(out, "tournament = {}", tm.to_string_lossy())?;
        }
        writeln!(out, "[auto_menu]")?;
        match &self.auto_menu {
            AutoMenu::Unused => (),
            AutoMenu::AutoMenu {
                name,
                race,
                game_name,
                connect_mode,
                lan_mode,
            } => {
                writeln!(out, "auto_menu=LAN")?;
                writeln!(out, "lan_mode={}", lan_mode)?;
                writeln!(out, "character_name={}", name)?;
                writeln!(out, "race={}", race)?;
                match connect_mode {
                    BwapiConnectMode::Host { map, player_count } => {
                        if let Some(map_name) = map {
                            writeln!(out, "map={}", map_name)?;
                        }
                        writeln!(out, "wait_for_min_players={}", player_count)?;
                        writeln!(out, "wait_for_max_players={}", player_count)?;
                    }
                    BwapiConnectMode::Join => {
                        writeln!(out, "game={}", game_name)?;
                    }
                }
            }
        }
        writeln!(
            out,
            "save_replay = replays/$Y $b $d/%MAP%_%BOTRACE%%ALLYRACES%vs%ENEMYRACES%_$H$M$S.rep"
        )?;
        writeln!(out, "[starcraft]")?;
        writeln!(out, "speed_override = {}", self.game_speed)?;
        let sound = if self.sound { "ON" } else { "OFF" };
        writeln!(out, "sound = {sound}")
    }
}

#[cfg(test)]
mod test {
    use crate::bwapi::BwapiVersion;
    use crate::bwapi::BwapiVersion::{Bwapi375, Bwapi412, Bwapi420, Bwapi440};
    use crc::{Crc, CRC_32_ISO_HDLC};

    #[test]
    fn test_crc() {
        let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
        let chksum = crc.checksum(
            std::fs::read("test-resources/BWAPI440.dll")
                .unwrap()
                .as_slice(),
        );
        assert_eq!(BwapiVersion::from_u32(chksum), Some(Bwapi440));
        let chksum = crc.checksum(
            std::fs::read("test-resources/BWAPI420.dll")
                .unwrap()
                .as_slice(),
        );
        assert_eq!(BwapiVersion::from_u32(chksum), Some(Bwapi420));
        let chksum = crc.checksum(
            std::fs::read("test-resources/BWAPI412.dll")
                .unwrap()
                .as_slice(),
        );
        assert_eq!(BwapiVersion::from_u32(chksum), Some(Bwapi412));
        let chksum = crc.checksum(
            std::fs::read("test-resources/BWAPI375.dll")
                .unwrap()
                .as_slice(),
        );
        assert_eq!(BwapiVersion::from_u32(chksum), Some(Bwapi375));
        // BWAPI 375 is a replacement for 374
        let chksum = crc.checksum(
            std::fs::read("test-resources/BWAPI374.dll")
                .unwrap()
                .as_slice(),
        );
        assert_eq!(BwapiVersion::from_u32(chksum), Some(Bwapi375));
    }
}
