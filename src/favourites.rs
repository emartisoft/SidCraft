// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

//! Optional "Favourite Top 100 SIDs" playlist.
//!
//! The list is kept here as the numbered text supplied by the user. Each line
//! has the form `<number>. <HVSC path>`; everything before the first `/` is
//! just list numbering and is discarded. The remaining path is appended to the
//! HVSC mirror URL to produce an M3U entry.

use crate::engine::hvsc::DEFAULT_HVSC_URL;
use crate::engine::playlist::{self, Playlist};
use std::fs;
use std::io;
use std::path::PathBuf;

/// Name shown in the playlist library and used for the M3U file.
pub const PLAYLIST_NAME: &str = "Favourite Top 100 SIDs";

/// The editorial top 100, numbered as provided.
pub const FAVOURITE_TOP_100: &str = "\
 1. /MUSICIANS/D/Daglish_Ben/Last_Ninja.sid
  2. /MUSICIANS/G/Galway_Martin/Wizball.sid
  3. /MUSICIANS/H/Hubbard_Rob/Sanxion.sid
  4. /MUSICIANS/H/Hubbard_Rob/International_Karate.sid
  5. /MUSICIANS/H/Hubbard_Rob/Commando.sid
  6. /MUSICIANS/T/Tel_Jeroen/Cybernoid_II.sid
  7. /MUSICIANS/H/Hubbard_Rob/Delta.sid
  8. /MUSICIANS/G/Galway_Martin/Rambo_First_Blood_Part_II.sid
  9. /MUSICIANS/H/Hubbard_Rob/Monty_on_the_Run.sid
 10. /MUSICIANS/G/Galway_Martin/Arkanoid.sid
 11. /MUSICIANS/H/Hubbard_Rob/Crazy_Comets.sid
 12. /MUSICIANS/T/Tel_Jeroen/Cybernoid.sid
 13. /MUSICIANS/O/Ouwehand_Reyn/Last_Ninja_3.sid
 14. /MUSICIANS/G/Gray_Matt/Last_Ninja_2.sid
 15. /MUSICIANS/G/Gray_Fred/Mutants.sid
 16. /MUSICIANS/H/Hubbard_Rob/Knucklebusters.sid
 17. /MUSICIANS/H/Hubbard_Rob/Auf_Wiedersehen_Monty.sid
 18. /MUSICIANS/C/Cooksey_Mark/Ghosts_n_Goblins.sid
 19. /MUSICIANS/H/Hubbard_Rob/Nemesis_the_Warlock.sid
 20. /MUSICIANS/G/Galway_Martin/Green_Beret.sid
 21. /MUSICIANS/B/Brennan_Neil/Way_of_the_Exploding_Fist.sid
 22. /MUSICIANS/D/Dunn_Jonathan/Platoon.sid
 23. /MUSICIANS/H/Huelsbeck_Chris/R-Type.sid
 24. /MUSICIANS/D/Daglish_Ben/Cobra.sid
 25. /MUSICIANS/H/Huelsbeck_Chris/Great_Giana_Sisters.sid
 26. /MUSICIANS/T/Tel_Jeroen/Myth.sid
 27. /MUSICIANS/F/Follin_Tim/Ghouls_n_Ghosts.sid
 28. /MUSICIANS/B/Bjerregaard_Johannes/Stormlord.sid
 29. /MUSICIANS/G/Galway_Martin/Parallax.sid
 30. /MUSICIANS/H/Hubbard_Rob/Lightforce.sid
 31. /MUSICIANS/D/Deenen_Charles/Zamzara.sid
 32. /MUSICIANS/G/Galway_Martin/Comic_Bakery.sid
 33. /MUSICIANS/T/Tel_Jeroen/Robocop_3.sid
 34. /MUSICIANS/G/Galway_Martin/Yie_Ar_Kung_Fu.sid
 35. /MUSICIANS/J/Joseph_Richard/Defender_of_the_Crown.sid
 36. /MUSICIANS/T/Tel_Jeroen/Turbo_Outrun.sid
 37. /MUSICIANS/H/Hubbard_Rob/Master_of_Magic.sid
 38. /MUSICIANS/C/Clarke_Peter/Ocean_Loader_3.sid
 39. /MUSICIANS/H/Hubbard_Rob/Spellbound.sid
 40. /MUSICIANS/D/Daglish_Ben/Trap.sid
 41. /MUSICIANS/B/Bjerregaard_Johannes/Sweet.sid
 42. /MUSICIANS/H/Huelsbeck_Chris/Antics_Dulcedo_Cogitationis.sid
 43. /MUSICIANS/H/Hubbard_Rob/IK_plus.sid
 44. /MUSICIANS/D/Daglish_Ben/Thing_Bounces_Back.sid
 45. /MUSICIANS/D/Dunn_Jonathan/RoboCop.sid
 46. /MUSICIANS/F/Future_Freak/Cooperation_Demo.sid
 47. /MUSICIANS/H/Hubbard_Rob/Zoids.sid
 48. /MUSICIANS/T/Tel_Jeroen/Supremacy.sid
 49. /MUSICIANS/G/Gray_Matt/Driller.sid
 50. /MUSICIANS/H/Hubbard_Rob/One_Man_and_His_Droid.sid
 51. /MUSICIANS/T/Tel_Jeroen/Savage.sid
 52. /MUSICIANS/H/Hubbard_Rob/ACE_II.sid
 53. /MUSICIANS/H/Hubbard_Rob/Thrust.sid
 54. /MUSICIANS/G/Gray_Fred/Hysteria.sid
 55. /MUSICIANS/H/Hubbard_Rob/BMX_Kidz.sid
 56. /MUSICIANS/Y/Yip/Scroll_Machine.sid
 57. /MUSICIANS/H/Huelsbeck_Chris/Baby_Of_Can_Guru.sid
 58. /MUSICIANS/H/Huelsbeck_Chris/To_Be_on_Top.sid
 59. /MUSICIANS/C/Clever_Music/Wizardry.sid
 60. /MUSICIANS/W/Walker_Martin/Armalyte.sid
 61. /MUSICIANS/F/Follin_Tim/Gauntlet_III.sid
 62. /MUSICIANS/H/Hatlelid_Kris/Grand_Prix_Circuit.sid
 63. /MUSICIANS/B/Bjerregaard_Johannes/Nightdawn.sid
 64. /MUSICIANS/F/Follin_Tim/Scumball.sid
 65. /MUSICIANS/B/Beben_Wally/R_I_S_K.sid
 66. /MUSICIANS/F/Future_Freak/Rocky_Star.sid
 67. /MUSICIANS/H/Hubbard_Rob/Mega_Apocalypse.sid
 68. /MUSICIANS/H/Huelsbeck_Chris/Katakis.sid
 69. /MUSICIANS/H/Hubbard_Rob/Gerry_the_Germ.sid
 70. /MUSICIANS/A/ATOO/Compleeto.sid
 71. /MUSICIANS/T/Tel_Jeroen/Eliminator.sid
 72. /MUSICIANS/D/Daglish_Ben/Bulldog.sid
 73. /MUSICIANS/C/Cooksey_Mark/Ramparts.sid
 74. /MUSICIANS/D/Daglish_Ben/Ark_Pandora.sid
 75. /MUSICIANS/H/Hubbard_Rob/Warhawk.sid
 76. /MUSICIANS/D/Daglish_Ben/Vikings.sid
 77. /MUSICIANS/F/FAME/Hendriks_Michael/Dynamic_Range.sid
 78. /MUSICIANS/G/Galway_Martin/Times_of_Lore.sid
 79. /MUSICIANS/J/JCH/Batman_long.sid
 80. /MUSICIANS/G/Galway_Martin/Mikie.sid
 81. /MUSICIANS/C/Crowther_Antony/Zig_Zag.sid
 82. /MUSICIANS/T/Tel_Jeroen/Battle_Valley.sid
 83. /MUSICIANS/J/JCH/Chordian.sid
 84. /MUSICIANS/T/Tel_Jeroen/Combat_Crazy.sid
 85. /MUSICIANS/T/Tel_Jeroen/Hawkeye.sid
 86. /MUSICIANS/G/Galway_Martin/Short_Circuit.sid
 87. /MUSICIANS/L/Laxity/DNA_Warrior.sid
 88. /MUSICIANS/W/Whittaker_David/Glider_Rider.sid
 89. /MUSICIANS/T/Tel_Jeroen/Kinetix.sid
 90. /MUSICIANS/T/Turner_Steve/Uridium.sid
 91. /MUSICIANS/B/Bjerregaard_Johannes/Thunderforce.sid
 92. /MUSICIANS/H/Hubbard_Rob/Kentilla.sid
 93. /MUSICIANS/B/Bjerregaard_Johannes/Crazy_Comets_remix.sid
 94. /MUSICIANS/C/Clarke_Peter/Mission_of_Mercy.sid
 95. /MUSICIANS/T/Tel_Jeroen/Afterburner.sid
 96. /MUSICIANS/B/Baldwin_Neil/Another_Tune_for_Joanne.sid
 97. /MUSICIANS/G/Galway_Martin/Neverending_Story.sid
 98. /MUSICIANS/G/Galway_Martin/Ocean_Loader_2.sid
 99. /MUSICIANS/W/Walker_Martin/Snare.sid
100. /MUSICIANS/B/Brennan_Neil/Fist_II-The_Legend_Continues.sid
";

/// Path of the managed playlist inside the playlist library.
pub fn playlist_path() -> PathBuf {
    playlist::playlists_dir().join(format!("{PLAYLIST_NAME}.m3u"))
}

/// Extracts the HVSC path from one numbered list line, dropping the leading
/// `number. ` prefix. Lines without a SID path are ignored.
fn hvsc_path(line: &str) -> Option<&str> {
    let path = line.find('/').map(|at| line[at..].trim())?;
    path.to_ascii_lowercase().ends_with(".sid").then_some(path)
}

/// Builds the playlist from the numbered list, prefixing each HVSC path with
/// the mirror URL.
pub fn build_playlist() -> Playlist {
    let mut result = Playlist::empty();
    for line in FAVOURITE_TOP_100.lines() {
        if let Some(path) = hvsc_path(line) {
            result.add(&format!("{DEFAULT_HVSC_URL}{path}"), None);
        }
    }
    result
}

/// The exact M3U text generated from the list.
fn m3u_content() -> String {
    build_playlist()
        .entries
        .iter()
        .map(|entry| format!("{}\n", entry.source))
        .collect()
}

/// Creates the M3U file when enabled, and removes it when disabled.
///
/// Existing content that already matches the list is left untouched, so an
/// app launch does not rewrite the file unnecessarily.
pub fn sync(enabled: bool) -> io::Result<()> {
    let path = playlist_path();
    if !enabled {
        return match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
    }

    let content = m3u_content();
    if fs::read_to_string(&path).ok().as_deref() == Some(content.as_str()) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    build_playlist().save(&path)
}

#[cfg(test)]
mod tests {
    use super::{FAVOURITE_TOP_100, build_playlist, hvsc_path};
    use crate::engine::hvsc::DEFAULT_HVSC_URL;

    #[test]
    fn list_has_one_hundred_sid_paths() {
        let playlist = build_playlist();
        assert_eq!(playlist.len(), 100);
        assert_eq!(FAVOURITE_TOP_100.lines().count(), 100);
    }

    #[test]
    fn numbering_is_ignored() {
        assert_eq!(
            hvsc_path(" 42. /MUSICIANS/H/Hubbard_Rob/Knucklebusters.sid"),
            Some("/MUSICIANS/H/Hubbard_Rob/Knucklebusters.sid")
        );
    }

    #[test]
    fn urls_use_the_hvsc_mirror() {
        let playlist = build_playlist();
        assert_eq!(
            playlist.entries[0].source,
            format!("{DEFAULT_HVSC_URL}/MUSICIANS/D/Daglish_Ben/Last_Ninja.sid")
        );
        assert_eq!(
            playlist.entries[99].source,
            format!("{DEFAULT_HVSC_URL}/MUSICIANS/B/Brennan_Neil/Fist_II-The_Legend_Continues.sid")
        );
    }
}
