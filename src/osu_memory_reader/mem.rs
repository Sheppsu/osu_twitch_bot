#[cfg(target_os = "windows")]
use crate::osu_memory_reader::win::*;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HANDLE;
#[cfg(target_os = "windows")]
use windows::Win32::System::Memory::{ PAGE_NOACCESS, PAGE_GUARD };
#[cfg(target_os = "windows")]
use windows::Win32::System::ProcessStatus::MODULEINFO;

use crate::osu_memory_reader::read::MemoryReader;

use std::path::PathBuf;

enum PatternValue {
    V(u8), // value
    A() // any
}

// make it easier to write out the patterns
use PatternValue::{ V, A };

#[derive(Debug)]
pub struct SettingsMemoryData {
    pub songs_folder: String,
    pub skin_folder: String,
    pub show_interface: i8
}

#[derive(Debug)]
pub struct TournamentMemoryData {
    pub ipc_state: i32,
    pub left_stars: i32,
    pub right_stars: i32,
    pub bo: i32,
    pub stars_visible: i8,
    pub score_visible: i8,
    pub team_one_name: String,
    pub team_two_name: String,
    pub team_one_score: i32,
    pub team_two_score: i32,
    pub ipc_base_addr: u32
}

#[derive(Debug)]
pub struct ResultsMemoryData {
    pub player_name: String,
    pub mods: u32,
    pub mode: i32,
    pub max_combo: i16,
    pub score: i32,
    pub hit100: i16,
    pub hit300: i16,
    pub hit50: i16,
    pub hit_geki: i16,
    pub hit_katu: i16,
    pub misses: i16,
    pub accuracy: f64
}

#[derive(Debug)]
pub struct MenuMemoryData {
    pub game_mode: i32,
    pub plays: i32,
    pub artist: String,
    pub artist_original: String,
    pub title: String,
    pub title_original: String,
    pub ar: f32,
    pub cs: f32,
    pub hp: f32,
    pub od: f32,
    pub audio_file: String,
    pub bg_file: String,
    pub folder: String,
    pub creator: String,
    pub name: String,
    pub path: String,
    pub difficulty: String,
    pub beatmap_id: i32,
    pub beatmapset_id: i32,
    pub ranked_status: i32,
    pub md5: String,
    pub object_count: i32,
    pub mods: u32
}

impl MenuMemoryData {
    pub fn status_name(&self) -> &'static str {
        match self.ranked_status {
            0 => "...", // unknown
            1 => "unsubmitted",
            2 => "unranked", // graveyard, wip, pending
            3 => "", // unused
            4 => "ranked",
            5 => "approved",
            6 => "qualified",
            7 => "loved",
            _ => ""
        }
    }
}

#[derive(Debug)]
pub struct GameplayMemoryData {
    pub stats: ResultsMemoryData,
    pub retries: i32,
    pub hit_errors: Vec<i32>,
    pub combo: i16,
    pub hp_smooth: f64,
    pub hp: f64,
    // pub leaderboard: u32,
    // keyoverlayarrayaddr
}

#[derive(Debug)]
pub struct MemoryData {
    pub status: u32,
    pub chat_status: i8,
    pub play_time: i32,
    pub settings: SettingsMemoryData,
    pub tournament: Option<TournamentMemoryData>,
    pub results: Option<ResultsMemoryData>,
    pub menu: MenuMemoryData,
    pub gameplay: Option<GameplayMemoryData>
}

impl MemoryData {
    pub fn current_mods(&self) -> u32 {
        if let Some(ref results) = self.results {
            results.mods
        } else if let Some(ref gameplay) = self.gameplay {
            gameplay.stats.mods
        } else {
            self.menu.mods
        }
    }
}

#[derive(Debug)]
#[derive(Default)]
pub struct AddressInfo {
    pub status: usize,
    pub settings_class: usize,
    pub base: usize,
    pub menu_mods: usize,
    pub play_time: usize,
    pub chat_checker: usize,
    pub skin_data: usize,
    pub rulesets: usize,
    pub chat_area: usize,
    pub user_info: usize
}

pub struct MemoryClient {
    pub proc_id: u32,
    hproc: HANDLE,
    pub hinfo: MODULEINFO,
    pub is_open: bool,
    addresses: AddressInfo,
    pub osu_path: PathBuf
}

impl MemoryClient {
    pub fn open() -> Result<Self, String> {
        unsafe {
            let proc_id = find_proc("osu!.exe")?;
            let hproc = open_process(proc_id)?;
            let (exe_path, mod_info) = get_proc_info(hproc)?;
            let mut osu_path = PathBuf::from(exe_path);
            osu_path.pop();

            Ok(MemoryClient {
                proc_id,
                hproc,
                hinfo: mod_info,
                is_open: true,
                addresses: AddressInfo::default(),
                osu_path
            })
        }
    }

    fn close(&mut self) -> Result<(), String> {
        if !self.is_open {
            return Err(String::from("MemoryClient has already been closed"));
        }
        unsafe { close_handle(self.hproc)? }
        self.is_open = false;
        Ok(())
    }

    pub fn init(&mut self) -> Result<(), String> {
        if !self.is_open {
            return Err(String::from("Cannot init after closing MemoryClient"));
        }

        let mut addr = 0;
        loop {
            let page = match unsafe { query_page(self.hproc, addr) } {
                Some(p) => p,
                None => break
            };

            let base_address = page.BaseAddress as usize;
            addr = base_address + page.RegionSize;

            if page.Protect.0 & PAGE_NOACCESS.0 > 0 || page.Protect.0 & PAGE_GUARD.0 > 0 || page.Protect.0 == 0 {
                continue;
            }

            let mut data = vec![0_u8; page.RegionSize];
            unsafe { read_address(self.hproc, base_address, &mut data, page.RegionSize)? }

            if self.search(data.as_slice(), base_address) {
                return Ok(());
            }
        }

        Err(String::from("Unable to find all patterns"))
    }

    fn search(&mut self, data: &[u8], base_address: usize) -> bool {
        let mut valid_count = 0_u8;
        let mut total_count = 0_u8;

        macro_rules! search_for {
            ( $n:expr; $p:expr ) => {
                {
                    total_count += 1;
                    if $n == 0 {
                        $n = match_pattern(data, $p, &mut valid_count, base_address);
                    } else {
                        valid_count += 1;
                    }
                }
            };
        }

        search_for!(self.addresses.status; &[V(0x48), V(0x83), V(0xF8), V(0x04), V(0x73), V(0x1E)]);
        search_for!(self.addresses.settings_class; &[V(0x83), V(0xE0), V(0x20), V(0x85), V(0xC0), V(0x7E), V(0x2F)]);
        search_for!(self.addresses.base; &[V(0xF8), V(0x1), V(0x74), V(0x04), V(0x83), V(0x65)]);
        search_for!(self.addresses.menu_mods; &[V(0xC8), V(0xFF), A(), A(), A(), A(), A(), V(0x81), V(0xD), A(), A(), A(), A(), V(0), V(0x8), V(0), V(0)]);
        search_for!(self.addresses.play_time; &[V(0x5E), V(0x5F), V(0x5D), V(0xC3), V(0xA1), A(), A(), A(), A(), V(0x89), A(), V(0x4)]);
        search_for!(self.addresses.chat_checker; &[V(0xA), V(0xD7), V(0x23), V(0x3C), V(0), V(0), A(), V(0x1)]);
        search_for!(self.addresses.skin_data; &[V(0x75), V(0x21), V(0x8B), V(0x1D)]);
        search_for!(self.addresses.rulesets; &[V(0x7D), V(0x15), V(0xA1), A(), A(), A(), A(), V(0x85), V(0xC0)]);
        search_for!(self.addresses.chat_area; &[V(0x33), V(0x47), V(0x9D), V(0xFF), V(0x5B), V(0x7F), V(0xFF), V(0xFF)]);
        // TODO: likely a more compoact way to do this
        if self.addresses.status == 0 || (
            match unsafe { self.read_u32(self.resolve_ptrs(self.addresses.status, &[-4])) } {
                Ok(status) => status == 22,
                Err(_e) => false
            }
        ) {
            search_for!(self.addresses.user_info; &[V(0x52), V(0x30), V(0x8B), V(0xC8), V(0xE8), A(), A(), A(), A(), V(0x8B), V(0xC8), V(0x8D)]);
        }
        

        return valid_count == total_count;
    }

    unsafe fn resolve_ptrs(&self, start: usize, offsets: &[isize]) -> usize {
        let mut addr = start;
        for offset in offsets {
            let offset = *offset;
            addr = addr.wrapping_add_signed(offset);

            addr = match self.read_ptr(addr) {
                Ok(a) => a,
                Err(_e) => {
                    addr = 0;
                    break;
                }
            };
        }
        addr
    }

    pub fn get_memory_data(&mut self) -> Result<MemoryData, String> {
        unsafe {
            let ruleset = self.resolve_ptrs(self.addresses.rulesets, &[-0xB, 0x4]);
            let settings = self.resolve_ptrs(self.addresses.settings_class, &[0x8]);
            let menu_base = self.resolve_ptrs(self.addresses.base, &[-0x33]);
            let beatmap = self.resolve_ptrs(self.addresses.base, &[-0xC]);
            let menu_beatmap = self.resolve_ptrs(beatmap, &[0]);

            let status = self.read_u32(self.resolve_ptrs(self.addresses.status, &[-4]))?;

            let mut tournament = None;
            let mut results = None;
            let mut gameplay = None;
            match status {
                // TODO: status enums
                2 => {
                    let gameplay_ruleset_base = self.resolve_ptrs(ruleset, &[0x68]);
                    let gameplay_ruleset1 = self.resolve_ptrs(gameplay_ruleset_base, &[0x38]);
                    let gameplay_ruleset2 = self.resolve_ptrs(gameplay_ruleset_base, &[0x40]);
                    let gameplay_mods = self.resolve_ptrs(gameplay_ruleset1, &[0x1C]);
                    gameplay = Some(
                        GameplayMemoryData {
                            stats: ResultsMemoryData {
                                player_name: self.read_str(self.resolve_ptrs(gameplay_ruleset1, &[0x28]))?,
                                mods: (self.read_i32(gameplay_mods + 0xC)? ^ self.read_i32(gameplay_mods + 0x8)?) as u32,
                                mode: self.read_i32(gameplay_ruleset1 + 0x64)?,
                                max_combo: self.read_i16(gameplay_ruleset1 + 0x68)?,
                                score: self.read_i32(ruleset + 0x100)?,
                                hit100: self.read_i16(gameplay_ruleset1 + 0x88)?,
                                hit300: self.read_i16(gameplay_ruleset1 + 0x8A)?,
                                hit50: self.read_i16(gameplay_ruleset1 + 0x8C)?,
                                hit_geki: self.read_i16(gameplay_ruleset1 + 0x8E)?,
                                hit_katu: self.read_i16(gameplay_ruleset1 + 0x90)?,
                                misses: self.read_i16(gameplay_ruleset1 + 0x92)?,
                                accuracy: self.read_f64(self.resolve_ptrs(ruleset, &[0x68, 0x48])+0xC)?
                            },
                            retries: self.read_i32(menu_base + 0x8)?,
                            hit_errors: self.read_array::<i32>(self.resolve_ptrs(gameplay_ruleset1, &[0x38, 0x4]))?,
                            combo: self.read_i16(gameplay_ruleset1 + 0x94)?,
                            hp_smooth: self.read_f64(gameplay_ruleset2 + 0x14)?,
                            hp: self.read_f64(gameplay_ruleset2 + 0x1C)?
                        }
                    )
                },
                7 => {
                    let result_ruleset = self.resolve_ptrs(ruleset, &[0x38]);
                    let result_mods = self.resolve_ptrs(result_ruleset, &[0x1C]);
                    results = Some(
                        ResultsMemoryData {
                            player_name: self.read_str(self.resolve_ptrs(result_ruleset, &[0x28]))?,
                            mods: (self.read_i32(result_mods + 0xC)? ^ self.read_i32(result_mods + 0x8)?) as u32,
                            mode: self.read_i32(result_ruleset + 0x64)?,
                            max_combo: self.read_i16(result_ruleset + 0x68)?,
                            score: self.read_i32(result_ruleset + 0x78)?,
                            hit100: self.read_i16(result_ruleset + 0x88)?,
                            hit300: self.read_i16(result_ruleset + 0x8A)?,
                            hit50: self.read_i16(result_ruleset + 0x8C)?,
                            hit_geki: self.read_i16(result_ruleset + 0x8E)?,
                            hit_katu: self.read_i16(result_ruleset + 0x90)?,
                            misses: self.read_i16(result_ruleset + 0x92)?,
                            accuracy: self.read_f64(self.resolve_ptrs(ruleset, &[0x48])+0xC)?
                        }
                    )
                },
                22 => {
                    let tourney_ruleset1 = self.resolve_ptrs(ruleset, &[0x1C]);
                    let tourney_ruleset2 = self.resolve_ptrs(ruleset, &[0x20]);
                    tournament = Some(
                        TournamentMemoryData {
                            ipc_state: self.read_i32(ruleset + 0x54)?,
                            left_stars: self.read_i32(tourney_ruleset1+0x2C)?,
                            right_stars: self.read_i32(tourney_ruleset2 + 0x2C)?,
                            bo: self.read_i32(tourney_ruleset2 + 0x30)?,
                            stars_visible: self.read_i8(tourney_ruleset2 + 0x38)?,
                            score_visible: self.read_i8(tourney_ruleset2 + 0x39)?,
                            team_one_name: self.read_str(self.resolve_ptrs(tourney_ruleset1, &[0x20, 0x144]))?,
                            team_two_name: self.read_str(self.resolve_ptrs(tourney_ruleset2, &[0x20, 0x144]))?,
                            team_one_score: self.read_i32(tourney_ruleset1 + 0x28)?,
                            team_two_score: self.read_i32(tourney_ruleset2 + 0x28)?,
                            ipc_base_addr: self.read_u32(self.resolve_ptrs(ruleset, &[0x34, 0x4])+0x4)?
                        }
                    )
                },
                _ => {}
            }

            Ok(MemoryData {
                status: status,
                chat_status: self.read_i8(self.addresses.chat_checker - 0x20)?,
                play_time: self.read_i32(self.resolve_ptrs(self.addresses.play_time, &[0x5]))?,
                settings: SettingsMemoryData {
                    songs_folder: self.read_str(self.resolve_ptrs(settings, &[0xB8, 0x4]))?,
                    skin_folder: self.read_str(self.resolve_ptrs(self.addresses.skin_data, &[0x4, 0, 68]))?,
                    show_interface: self.read_i8(self.resolve_ptrs(settings, &[0x4])+0xC)?
                },
                tournament: tournament,
                results: results,
                menu: MenuMemoryData {
                    game_mode: self.read_i32(menu_base)?,
                    plays: self.read_i32(menu_base + 0xC)?,
                    artist: self.read_str(self.resolve_ptrs(menu_beatmap, &[0x18]))?,
                    artist_original: self.read_str(self.resolve_ptrs(menu_beatmap, &[0x1C]))?,
                    title: self.read_str(self.resolve_ptrs(menu_beatmap, &[0x24]))?,
                    title_original: self.read_str(self.resolve_ptrs(menu_beatmap, &[0x28]))?,
                    ar: self.read_f32(menu_beatmap + 0x2C)?,
                    cs: self.read_f32(menu_beatmap + 0x30)?,
                    hp: self.read_f32(menu_beatmap + 0x34)?,
                    od: self.read_f32(menu_beatmap + 0x38)?,
                    audio_file: self.read_str(self.resolve_ptrs(menu_beatmap, &[0x64]))?,
                    bg_file: self.read_str(self.resolve_ptrs(menu_beatmap, &[0x68]))?,
                    folder: self.read_str(self.resolve_ptrs(menu_beatmap, &[0x78]))?,
                    creator: self.read_str(self.resolve_ptrs(menu_beatmap, &[0x7C]))?,
                    name: self.read_str(self.resolve_ptrs(menu_beatmap, &[0x80]))?,
                    path: self.read_str(self.resolve_ptrs(menu_beatmap, &[0x90]))?,
                    difficulty: self.read_str(self.resolve_ptrs(menu_beatmap, &[0xAC]))?,
                    beatmap_id: self.read_i32(menu_beatmap + 0xC8)?,
                    beatmapset_id: self.read_i32(menu_beatmap + 0xCC)?,
                    ranked_status: self.read_i32(menu_beatmap + 0x12C)?,
                    md5: self.read_str(self.resolve_ptrs(menu_beatmap, &[0x6C]))?,
                    object_count: self.read_i32(menu_beatmap + 0xFC)?,
                    mods: self.read_u32(self.resolve_ptrs(self.addresses.menu_mods, &[0x9]))?
                },
                gameplay: gameplay
            })
        }
    }
}

impl MemoryReader for MemoryClient {
    fn handle(&self) -> HANDLE {
        self.hproc
    }
}

impl Drop for MemoryClient {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn match_pattern(data: &[u8], pattern: &[PatternValue], valid_count: &mut u8, base_address: usize) -> usize {
    let mut pi = 0_usize;
    for i in 0..data.len() {
        if !match pattern[pi] {
            V(v) => data[i] == v,
            A() => true
        } {
            pi = 0;
            continue;
        }

        pi += 1;
        if pi == pattern.len() {
            *valid_count += 1;
            return base_address+i+1-pattern.len();
        }
    }
    return 0;
}