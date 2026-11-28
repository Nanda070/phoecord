//! Voice and audio (task 2.3.16), the pure parts. So far VolumeBooster (Vencord: VolumeBooster,
//! by Nuckyz, sadan and Vendicated, GPL-3.0-or-later): a higher ceiling for a user's and a
//! stream's volume.
//!
//! Phoenix clamps a volume (percent, `f32`) in two functions, one for users and one for streams,
//! each with `movss xmm6, [rip+disp32]` loading the ceiling `200.0` (private recon notes,
//! 2.3.16). The literal is shared by many other places, so it is never changed: the hook
//! (`hooks::volume`) points those two instructions at a value of PhoeCord's own.
//!
//! ```toml
//! [plugins.volume_booster]
//! enabled = true
//! multiplier = 2   # 1 to 10: the ceiling is 200 % times this
//! ```
//!
//! **Only the clamp is lifted.** The client's own slider still ends at 200 %: where it gets that
//! maximum is not located yet.

use phoecord_transport::config::Config;

/// The plugin's table in `config.toml`.
pub const ID: &str = "volume_booster";
/// The client's own ceiling, in percent.
pub const NATIVE_CEILING: f32 = 200.0;
/// Vencord's default multiplier.
pub const DEFAULT_MULTIPLIER: f64 = 2.0;
/// The owner's limit for the feature: ten times the client's ceiling.
pub const MAX_MULTIPLIER: f64 = 10.0;

/// The ceiling the user asked for, in percent: `None` unless the plugin is on and the ceiling
/// would be above the client's own.
pub fn ceiling(cfg: &Config) -> Option<f32> {
    if !cfg.is_enabled(ID) {
        return None;
    }
    let multiplier = cfg
        .plugins
        .get(ID)
        .and_then(|t| t.get("multiplier"))
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
        .filter(|m| m.is_finite())
        .unwrap_or(DEFAULT_MULTIPLIER)
        .clamp(1.0, MAX_MULTIPLIER);
    let ceiling = NATIVE_CEILING * multiplier as f32;
    (ceiling > NATIVE_CEILING).then_some(ceiling)
}

/// The first bytes of `movss xmm6, [rip+disp32]`.
const LOAD_CEILING: [u8; 4] = [0xF3, 0x0F, 0x10, 0x35];
/// The whole instruction: opcode and `disp32`.
pub const INSTRUCTION_LEN: usize = 8;

/// The address the ceiling is loaded from, if `code` at address `at` is the expected load.
pub fn loaded_from(code: [u8; INSTRUCTION_LEN], at: usize) -> Option<usize> {
    if code[..4] != LOAD_CEILING {
        return None;
    }
    let disp = i32::from_le_bytes([code[4], code[5], code[6], code[7]]);
    (at + INSTRUCTION_LEN).checked_add_signed(disp as isize)
}

/// The `disp32` that makes the instruction at `at` load from `value_at`, if it is in reach.
pub fn displacement(at: usize, value_at: usize) -> Option<[u8; 4]> {
    let next = at.checked_add(INSTRUCTION_LEN)?;
    let delta = (value_at as i128) - (next as i128);
    i32::try_from(delta).ok().map(i32::to_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(multiplier: Option<&str>, enabled: bool) -> Config {
        let mut text = format!("[plugins.volume_booster]\nenabled = {enabled}\n");
        if let Some(m) = multiplier {
            text.push_str(&format!("multiplier = {m}\n"));
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.toml");
        std::fs::write(&file, text).unwrap();
        Config::load(&file).unwrap()
    }

    #[test]
    fn off_until_enabled_then_twice_the_client_ceiling() {
        assert_eq!(ceiling(&Config::default()), None);
        assert_eq!(ceiling(&config(Some("5"), false)), None);
        assert_eq!(ceiling(&config(None, true)), Some(400.0));
    }

    #[test]
    fn the_multiplier_is_bounded() {
        assert_eq!(ceiling(&config(Some("5"), true)), Some(1000.0));
        assert_eq!(ceiling(&config(Some("1.5"), true)), Some(300.0));
        assert_eq!(ceiling(&config(Some("1000"), true)), Some(2000.0));
        // Nothing to lift.
        assert_eq!(ceiling(&config(Some("1"), true)), None);
        assert_eq!(ceiling(&config(Some("-3"), true)), None);
        // Not a number: the default.
        assert_eq!(ceiling(&config(Some("\"loud\""), true)), Some(400.0));
        assert_eq!(ceiling(&config(Some("nan"), true)), Some(400.0));
    }

    #[test]
    fn the_load_is_recognised_and_repointed() {
        let at = 0x1000usize;
        let mut code = [0xF3, 0x0F, 0x10, 0x35, 0x10, 0, 0, 0];
        assert_eq!(loaded_from(code, at), Some(0x1018));
        code[4..].copy_from_slice(&displacement(at, 0x800).unwrap());
        assert_eq!(loaded_from(code, at), Some(0x800));
        // Another register, another instruction.
        code[3] = 0x05;
        assert_eq!(loaded_from(code, at), None);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(displacement(at, 0x1_0000_0000), None);
    }
}
