//! User settings, defaults from MECHANICS.md (0x408b40). Stored as key=value text.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub density: i32,
    pub bias: i32,
    pub folder_color: i32,
    pub file_color: i32,
    pub show_name_tips: bool,
    pub nametip_delay: u32,
    pub show_info_tips: bool,
    pub infotip_delay: u32,
    pub infotip_flags: u32,
    pub show_rollover_box: bool,
    pub save_pos: bool,
    pub animated_zoom: bool,
    pub disable_delete: bool,
    pub auto_rescan: bool,
    pub window: Option<[f32; 4]>,
}

/// Info-tip fields (our own bit order; the original's is not decoded yet).
pub const TIP_FULL_PATH: u32 = 1;
pub const TIP_FILENAME: u32 = 2;
pub const TIP_DATE: u32 = 4;
pub const TIP_SIZE: u32 = 8;
pub const TIP_ATTRS: u32 = 16;

pub const DENSITY_LABELS: [&str; 6] =
    ["Too Few Files", "Very Few Files", "Normal", "Lots of Files", "Very Many Files", "Too Many Files"];
pub const COLOR_LABELS: [&str; 14] = [
    "Rainbow",
    "Spectrum", // 0.3 — muted modern cousin of Rainbow
    "Pastel",   // 0.3 — Material 3–style pastel containers
    "Windows Colors",
    "White",
    "Light Gray",
    "Dark Gray",
    "Red",
    "Orange",
    "Yellow",
    "Green",
    "Aqua",
    "Blue",
    "Violet",
];
pub const COLOR_MAX: i32 = 13;

impl Default for Settings {
    fn default() -> Self {
        Settings {
            density: 1, // Very Many Files
            bias: 0,
            folder_color: 0,
            file_color: 0,
            show_name_tips: true,
            nametip_delay: 125,
            show_info_tips: true,
            infotip_delay: 250,
            infotip_flags: TIP_FULL_PATH | TIP_DATE | TIP_SIZE,
            show_rollover_box: false,
            save_pos: false,
            animated_zoom: true,
            disable_delete: false,
            auto_rescan: false,
            window: None,
        }
    }
}

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("kspacemonger").join("settings.conf"))
}

impl Settings {
    pub fn load() -> Settings {
        let mut s = Settings::default();
        let Some(p) = path() else { return s };
        let Ok(text) = std::fs::read_to_string(p) else { return s };
        let mut conf_ver = 1i64;
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let i = v.parse::<i64>().ok();
            let b = v == "1" || v == "true";
            match k {
                "version" => conf_ver = i.unwrap_or(1),
                "density" => s.density = i.unwrap_or(0).clamp(-3, 2) as i32,
                "bias" => s.bias = i.unwrap_or(0).clamp(-20, 20) as i32,
                "folder_color" => s.folder_color = i.unwrap_or(0).clamp(0, COLOR_MAX as i64) as i32,
                "file_color" => s.file_color = i.unwrap_or(0).clamp(0, COLOR_MAX as i64) as i32,
                "show_name_tips" => s.show_name_tips = b,
                "nametip_delay" => s.nametip_delay = i.unwrap_or(125).clamp(0, 5000) as u32,
                "show_info_tips" => s.show_info_tips = b,
                "infotip_delay" => s.infotip_delay = i.unwrap_or(250).clamp(0, 5000) as u32,
                "infotip_flags" => s.infotip_flags = i.unwrap_or(0) as u32,
                "show_rollover_box" => s.show_rollover_box = b,
                "save_pos" => s.save_pos = b,
                "animated_zoom" => s.animated_zoom = b,
                "disable_delete" => s.disable_delete = b,
                "auto_rescan" => s.auto_rescan = b,
                "window" => {
                    let v: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                    if v.len() == 4 {
                        s.window = Some([v[0], v[1], v[2], v[3]]);
                    }
                }
                _ => {}
            }
        }
        // v1→v2: Spectrum inserted at index 1; shift Windows/solids up by one.
        if conf_ver < 2 {
            if s.folder_color >= 1 {
                s.folder_color = (s.folder_color + 1).min(COLOR_MAX);
            }
            if s.file_color >= 1 {
                s.file_color = (s.file_color + 1).min(COLOR_MAX);
            }
        }
        // v2→v3: Pastel inserted at index 2. Old Spectrum (1) was the M3 pastels → Pastel (2).
        if conf_ver < 3 {
            let bump = |c: i32| -> i32 {
                if c == 1 {
                    2
                } else if c >= 2 {
                    (c + 1).min(COLOR_MAX)
                } else {
                    c
                }
            };
            s.folder_color = bump(s.folder_color);
            s.file_color = bump(s.file_color);
        }
        s
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let b = |x: bool| if x { 1 } else { 0 };
        let mut t = format!(
            "version=3\ndensity={}\nbias={}\nfolder_color={}\nfile_color={}\nshow_name_tips={}\nnametip_delay={}\nshow_info_tips={}\ninfotip_delay={}\ninfotip_flags={}\nshow_rollover_box={}\nsave_pos={}\nanimated_zoom={}\ndisable_delete={}\nauto_rescan={}\n",
            self.density,
            self.bias,
            self.folder_color,
            self.file_color,
            b(self.show_name_tips),
            self.nametip_delay,
            b(self.show_info_tips),
            self.infotip_delay,
            self.infotip_flags,
            b(self.show_rollover_box),
            b(self.save_pos),
            b(self.animated_zoom),
            b(self.disable_delete),
            b(self.auto_rescan)
        );
        if let Some(w) = self.window {
            t.push_str(&format!("window={},{},{},{}\n", w[0], w[1], w[2], w[3]));
        }
        let _ = std::fs::write(p, t);
    }
}
