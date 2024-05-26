mod osu_memory_reader;

use osu_memory_reader::mem::{MemoryClient, MemoryData};
use irc::client::prelude::*;
use futures::prelude::*;
use std::io::{Read, Write};
use std::time::{Duration, Instant};
use std::fs::File;

#[derive(Default)]
struct TwitchConfig {
    username: String,
    oauth_token: String,
    channel: String
}

impl TwitchConfig {
    pub fn new() -> Result<Self, String> {
        let mut f = match File::open("setup.cfg") {
            Ok(f) => f,
            Err(_) => {
                let mut f = File::create("setup.cfg").or(Err("Unable to create setup.cfg file"))?;
                f.write("USERNAME=\nOAUTH_TOKEN=\nCHANNEL=".as_bytes()).or(Err("Unable to write to setup.cfg"))?;
                return Err("setup.cfg has been created, so now enter info into it".into());
            }
        };

        let mut contents = Vec::new();
        let mut buf: [u8; 1024] = [0; 1024];
        loop {
            let nb = f.read(&mut buf).or(Err("Unable to read from setup.cfg"))?;
            if nb == 0 {
                break;
            }
            contents.extend_from_slice(&buf[..nb]);
        }

        let mut config = TwitchConfig::default();
        for line in String::from_utf8(contents).or(Err("Unable to parse contents of setup.cfg"))?.split("\n") {
            if line.trim().len() == 0 { continue; }

            let (key, value) = line.split_once("=").ok_or(String::from("Invalid formatting in setup.cfg"))?;
            let key = key.trim();
            let value = value.trim();

            if value.len() == 0 {
                return Err(format!("{key} is missing a value"));
            }

            match key.to_uppercase().as_str() {
                "USERNAME" => config.username = value.into(),
                "OAUTH_TOKEN" => config.oauth_token = value.into(),
                "CHANNEL" => config.channel = if value.starts_with("#") { value.into() } else { format!("#{}", value) },
                _ => return Err(format!("Invalid config key '{}'", key))
            };
        }

        Ok(config)
    }
}

macro_rules! transpose_err {
    ($r:expr) => {
        match $r {
            Ok(value) => Ok(value),
            Err(e) => Err(e.to_string())
        }
    };
}

const MOD_ABBREVIATIONS: [&'static str; 31] = [
    "NF",
    "EZ",
    "TD",
    "HD",
    "HR",
    "SD",
    "DT",
    "RX",
    "HT",
    "NC",
    "FL",
    "AU",
    "SO",
    "AP",
    "PF",
    "K4",
    "K5",
    "K6",
    "K7",
    "K8",
    "FI",
    "RA",
    "CN",
    "TP",
    "K9",
    "CO",
    "K1",
    "K3",
    "K2",
    "V2",
    "MR"
];

fn get_mod_string(mods: u32) -> String {
    if mods == 0 {
        return "".into();
    }

    let mut mod_strings: Vec<&str> = vec![];
    let mut value = mods;
    let mut nc_enabled = false;
    let nmods = MOD_ABBREVIATIONS.len();
    for i in 1..nmods+1 {
        let flag = 1 << (nmods - i);
        if value >= flag {
            value %= flag;

            // don't show dt if nc and dt are present
            if flag == 512 {
                nc_enabled = true;
            } else if flag == 64 && nc_enabled {
                if value == 0 { break } else { continue }
            }
            
            mod_strings.push(MOD_ABBREVIATIONS[nmods - i]);
            if value == 0 {
                break;
            }
        }
    }

    mod_strings.reverse();
    mod_strings.join("")
}

async fn get_data(client: &mut MemoryClient) -> Result<MemoryData, String> {
    for i in 0..5 {
        match client.get_memory_data() {
            Ok(data) => return Ok(data),
            Err(msg) => if i == 4 {
                return Err(msg);
            } else {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                continue;
            }
        };
    }

    unreachable!();
}

fn get_beatmap(client: &MemoryClient, data: &MemoryData) -> Result<rosu_pp::Beatmap, String> {
    let mut beatmap_path = client.osu_path.clone();
    beatmap_path.push(&data.settings.songs_folder);
    beatmap_path.push(&data.menu.folder);
    beatmap_path.push(&data.menu.path);

    let beatmap = match rosu_pp::Beatmap::from_path(beatmap_path) {
        Ok(bm) => bm,
        Err(_) => {
            return Err("Failed to parse beatmap file, likely due to permission issues".into())
        }
    };

    return Ok(beatmap);
}

fn parse_acc_arg(arg: &str) -> Result<f64, String> {
    match (if arg.ends_with("%") { &arg[..arg.len()-1] } else { arg }).parse::<f64>() {
        Ok(value) => if value < 0.0 || value > 100.0 {
            Err("Acc must be between 0 and 100".into())
        } else {
            Ok(value)
        },
        Err(_) => Err("Invalid acc format".into())
    }
}

fn parse_mod_arg(arg: &str) -> Result<u32, String> {
    let mods = if arg.starts_with("+") { &arg[1..] } else { arg };
    if mods.len() % 2 != 0 {
        return Err("Invalid mod abbreviation".into());
    }

    let mods = mods.to_uppercase();
    let mut mod_flags = 0;
    for i in 0..(mods.len() / 2) {
        let abbr = &mods[i*2..i*2+2];
        if abbr.eq("NM") { continue; }
        if let Some(i) = MOD_ABBREVIATIONS.iter().position(|&m| m.eq(abbr)) {
            mod_flags += 1 << i;
        } else {
            return Err("Invalid mod abbreviation".into());
        }
    }

    // add dt if nc is present without it
    if mod_flags & 512 != 0 && mod_flags & 64 == 0 {
        mod_flags += 64;
    }

    return Ok(mod_flags)
}

macro_rules! return_err_as_ok {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(e) => return Ok(e)
        }
    };
}

macro_rules! maybe_mods {
    ($mods:expr) => {
        if $mods == 0 { "".into() } else { format!(" +{}", get_mod_string($mods)) }
    };
}

async fn get_pp_now_text(client: &mut MemoryClient) -> Result<String, String> {
    let data = get_data(client).await?;
    if data.gameplay.is_none() {
        return Ok("Not playing anything".into());
    }

    let gameplay = data.gameplay.as_ref().unwrap();
    let stats = &gameplay.stats;
    let state = rosu_pp::any::ScoreState {
        n300: stats.hit300 as u32,
        n_geki: stats.hit_geki as u32,
        n100: stats.hit100 as u32,
        n_katu: stats.hit_katu as u32,
        n50: stats.hit50 as u32,
        misses: stats.misses as u32,
        max_combo: stats.max_combo as u32
    };

    let nhitobjects = (state.n300 + state.n100 + state.n50 + state.misses) as usize;
    if nhitobjects == 0 {
        return Ok("Current pp count: 0pp".into());
    }

    let beatmap = get_beatmap(client, &data)?;
    let mut gradual = rosu_pp::Difficulty::new().mods(stats.mods).gradual_performance(&beatmap);
    match gradual.nth(state, nhitobjects - 1) {
        Some(attrs) => Ok(format!("Current pp count: {:.2}", attrs.pp())),
        None => Ok("Current pp count: N/A".into())
    }
}

async fn get_pp_text(client: &mut MemoryClient, msg: &str) -> Result<String, String> {
    let data = get_data(client).await?;
    let mut acc = if data.gameplay.is_none() { 100.0 } else { data.gameplay.as_ref().unwrap().stats.accuracy };
    let mut mods = data.current_mods();

    let mut args = msg.split(" ");
    let mut acc_specified = false;
    args.next();
    for arg in args {
        if arg.starts_with("+") {
            mods = return_err_as_ok!(parse_mod_arg(arg));
        } else {
            acc = return_err_as_ok!(parse_acc_arg(arg));
            acc_specified = true;
        }
    }
    
    let beatmap = get_beatmap(client, &data)?;
    let difficulty = rosu_pp::Difficulty::new().mods(mods).calculate(&beatmap);
    // results screen vs other
    if acc_specified || data.results.is_none() {
        let pp = difficulty.performance().mods(mods).accuracy(acc).calculate().pp();
        Ok(format!(
            "[{}] {:.2} for {:.2}%{}",
            data.menu.status_name(),
            pp,
            acc,
            maybe_mods!(mods)
        ))
    } else {
        let results = data.results.as_ref().unwrap();
        let pp = difficulty.performance().mods(mods)
            .n300(results.hit300 as u32)
            .n100(results.hit100 as u32)
            .n50(results.hit50 as u32)
            .misses(results.misses as u32)
            .combo(results.max_combo as u32)
            .calculate().pp();
        Ok(format!(
            "[{}] {:.2} for this score{}",
            data.menu.status_name(),
            pp,
            if mods == 0 { "".into() } else { format!(" ({})", get_mod_string(mods)) }
        ))
    }
}

async fn get_np_text(client: &mut MemoryClient) -> Result<String, String> {
    let data = get_data(client).await?;
    let mods = data.current_mods();
    let beatmap = get_beatmap(client, &data)?;
    let difficulty = rosu_pp::Difficulty::new().mods(mods).calculate(&beatmap);

    Ok(format!(
        "Now playing: [{}] {} - {} [{}]{} ({:.2}*) by {} | https://osu.ppy.sh/b/{}",
        data.menu.status_name(),
        data.menu.artist,
        data.menu.title,
        data.menu.difficulty,
        maybe_mods!(mods),
        difficulty.stars(),
        data.menu.creator,
        data.menu.beatmap_id
    ))
}

struct Cooldowns([Instant; 3]);

impl Cooldowns {
    pub fn new() -> Self {
        return Self([Instant::now().checked_sub(Duration::from_secs(5)).unwrap(); 3])
    }

    pub fn can_use(&self, i: usize, cooldown: u64) -> bool {
        Instant::now().duration_since(self.0[i]).ge(&Duration::from_secs(cooldown))
    }

    pub fn reset(&mut self, i: usize) {
        self.0[i] = Instant::now();
    }
}

fn has_badge(msg: &Message, names: &[&str]) -> bool {
    for tag in msg.tags.as_ref().unwrap() {
        if tag.0.eq("badges") {
            for badge in tag.1.as_ref().unwrap().split(",") {
                let (name, _) = badge.split_once("/").or(Some((badge, ""))).unwrap();
                for n in names {
                    if name.eq(*n) {
                        return true;
                    }
                }
            }
        }
    }
    
    false
}

async fn run(mem_client: &mut MemoryClient, config: &TwitchConfig) -> Result<(), String> {
    println!("Connecting to server as {} and joining {}...", &config.username, &config.channel);

    let mut twitch_client = transpose_err!(Client::from_config(Config {
        nickname: Some(config.username.clone()),
        server: Some("irc.chat.twitch.tv".into()),
        channels: vec![config.channel.clone()],
        password: Some(config.oauth_token.clone()),
        ..Config::default()
    }).await)?;
    transpose_err!(twitch_client.identify())?;

    let sender = twitch_client.sender();
    transpose_err!(sender.send_cap_req(&[Capability::Custom("twitch.tv/tags")]))?;

    println!(
        "Connected to server as {}. If the bot successfully joins the channel, you should see a message saying so.",
        &config.username,
    );

    let mut stream = transpose_err!(twitch_client.stream())?;
    let mut cooldowns = Cooldowns::new();
    while let Some(msg) = transpose_err!(stream.next().await.transpose())? {
        if let Command::PRIVMSG(ref target, ref text) = msg.command {
            if !text.starts_with("!") { continue; }
            let (cmd, _) = text.split_once(" ").or(Some((text.trim(), ""))).unwrap();

            macro_rules! create_branch {
                ($i:literal, $c:literal, $f:expr) => {  // index, cooldown (seconds), function
                    {
                        if !cooldowns.can_use($i, $c) { continue; }
                        transpose_err!(sender.send_privmsg(target, $f.await?))?;
                        cooldowns.reset($i);
                    }
                };
                (modonly; $i:literal, $c:literal, $f:expr) => {
                    {
                        if !has_badge(&msg, &["moderator", "broadcaster"]) { continue; }
                        create_branch!($i, $c, $f)
                    }
                };
                (subonly; $i:literal, $c:literal, $f:expr) => {
                    {
                        if !has_badge(&msg, &["subscriber", "moderator", "broadcaster"]) { continue; }
                        create_branch!($i, $c, $f);
                    }
                };
            }
            match cmd {
                "!np" => create_branch!(0, 5, get_np_text(mem_client)),
                "!pp" => create_branch!(subonly; 1, 3, get_pp_text(mem_client, text)),
                "!ppnow" => create_branch!(modonly; 2, 1, get_pp_now_text(mem_client)),
                _ => {}
            }
        } else if let Command::JOIN(ref channel, _, _) = msg.command {
            println!("Successfully joined {}", channel);
        }
    }

    Ok(())
}

async fn start(config: &TwitchConfig) -> Result<(), String> {
    println!("Starting up...");
    let mut mem_client = MemoryClient::open()?;
    mem_client.init()?;

    run(&mut mem_client, config).await
}

async fn get_twitch_config() -> TwitchConfig {
    println!("Parsing config...");
    loop {
        match TwitchConfig::new() {
            Ok(config) => return config,
            Err(msg) => println!("{}. Trying again in 5 seconds...", msg)
        };
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let twitch_config = get_twitch_config().await;
    loop {
        if let Err(msg) = start(&twitch_config).await {
            println!("{}", msg);
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
