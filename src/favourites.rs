// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

//! Optional "Favourite Top 100 SIDs" playlist.
//!
//! The list contains curated HVSC paths for the editorial top 100 SIDs.
//! Each path is appended to the HVSC mirror URL to produce an M3U entry.

use crate::engine::hvsc::mirror_resource_url;
use crate::engine::playlist::{self, Playlist};
use std::fs;
use std::io;
use std::path::PathBuf;

/// Name shown in the playlist library and used for the M3U file.
pub const PLAYLIST_NAME: &str = "Favourite Top 100 SIDs";

/// The editorial top 100 SID paths.
pub const FAVOURITE_TOP_100: &[&str] = &[
    "/MUSICIANS/D/Daglish_Ben/Last_Ninja.sid",
    "/MUSICIANS/G/Galway_Martin/Wizball.sid",
    "/MUSICIANS/H/Hubbard_Rob/Sanxion.sid",
    "/MUSICIANS/H/Hubbard_Rob/International_Karate.sid",
    "/MUSICIANS/H/Hubbard_Rob/Commando.sid",
    "/MUSICIANS/T/Tel_Jeroen/Cybernoid_II.sid",
    "/MUSICIANS/H/Hubbard_Rob/Delta.sid",
    "/MUSICIANS/G/Galway_Martin/Rambo_First_Blood_Part_II.sid",
    "/MUSICIANS/H/Hubbard_Rob/Monty_on_the_Run.sid",
    "/MUSICIANS/G/Galway_Martin/Arkanoid.sid",
    "/MUSICIANS/H/Hubbard_Rob/Crazy_Comets.sid",
    "/MUSICIANS/T/Tel_Jeroen/Cybernoid.sid",
    "/MUSICIANS/O/Ouwehand_Reyn/Last_Ninja_3.sid",
    "/MUSICIANS/G/Gray_Matt/Last_Ninja_2.sid",
    "/MUSICIANS/G/Gray_Fred/Mutants.sid",
    "/MUSICIANS/H/Hubbard_Rob/Knucklebusters.sid",
    "/MUSICIANS/H/Hubbard_Rob/Auf_Wiedersehen_Monty.sid",
    "/MUSICIANS/C/Cooksey_Mark/Ghosts_n_Goblins.sid",
    "/MUSICIANS/H/Hubbard_Rob/Nemesis_the_Warlock.sid",
    "/MUSICIANS/G/Galway_Martin/Green_Beret.sid",
    "/MUSICIANS/B/Brennan_Neil/Way_of_the_Exploding_Fist.sid",
    "/MUSICIANS/D/Dunn_Jonathan/Platoon.sid",
    "/MUSICIANS/H/Huelsbeck_Chris/R-Type.sid",
    "/MUSICIANS/D/Daglish_Ben/Cobra.sid",
    "/MUSICIANS/H/Huelsbeck_Chris/Great_Giana_Sisters.sid",
    "/MUSICIANS/T/Tel_Jeroen/Myth.sid",
    "/MUSICIANS/F/Follin_Tim/Ghouls_n_Ghosts.sid",
    "/MUSICIANS/B/Bjerregaard_Johannes/Stormlord.sid",
    "/MUSICIANS/G/Galway_Martin/Parallax.sid",
    "/MUSICIANS/H/Hubbard_Rob/Lightforce.sid",
    "/MUSICIANS/D/Deenen_Charles/Zamzara.sid",
    "/MUSICIANS/G/Galway_Martin/Comic_Bakery.sid",
    "/MUSICIANS/T/Tel_Jeroen/RoboCop_3.sid",
    "/MUSICIANS/G/Galway_Martin/Yie_Ar_Kung_Fu.sid",
    "/MUSICIANS/J/Joseph_Richard/Defender_of_the_Crown.sid",
    "/MUSICIANS/T/Tel_Jeroen/Turbo_Outrun.sid",
    "/MUSICIANS/H/Hubbard_Rob/Master_of_Magic.sid",
    "/MUSICIANS/C/Clarke_Peter/Ocean_Loader_3.sid",
    "/MUSICIANS/H/Hubbard_Rob/Spellbound.sid",
    "/MUSICIANS/D/Daglish_Ben/Trap.sid",
    "/MUSICIANS/B/Bjerregaard_Johannes/Sweet.sid",
    "/MUSICIANS/H/Huelsbeck_Chris/Antics_Dulcedo_Cogitationis.sid",
    "/MUSICIANS/H/Hubbard_Rob/IK_plus.sid",
    "/MUSICIANS/D/Daglish_Ben/Thing_Bounces_Back.sid",
    "/MUSICIANS/D/Dunn_Jonathan/RoboCop.sid",
    "/MUSICIANS/F/Future_Freak/Cooperation_Demo.sid",
    "/MUSICIANS/H/Hubbard_Rob/Zoids.sid",
    "/MUSICIANS/T/Tel_Jeroen/Supremacy.sid",
    "/MUSICIANS/G/Gray_Matt/Driller.sid",
    "/MUSICIANS/H/Hubbard_Rob/One_Man_and_his_Droid.sid",
    "/MUSICIANS/T/Tel_Jeroen/Savage.sid",
    "/MUSICIANS/H/Hubbard_Rob/ACE_II.sid",
    "/MUSICIANS/H/Hubbard_Rob/Thrust.sid",
    "/MUSICIANS/G/Gray_Fred/Hysteria.sid",
    "/MUSICIANS/H/Hubbard_Rob/BMX_Kidz.sid",
    "/MUSICIANS/Y/Yip/Scroll_Machine.sid",
    "/MUSICIANS/H/Huelsbeck_Chris/Baby_of_Can_Guru.sid",
    "/MUSICIANS/H/Huelsbeck_Chris/To_be_on_Top.sid",
    "/MUSICIANS/C/Clever_Music/Wizardry.sid",
    "/MUSICIANS/W/Walker_Martin/Armalyte.sid",
    "/MUSICIANS/F/Follin_Tim/Gauntlet_III.sid",
    "/MUSICIANS/H/Hatlelid_Kris/Grand_Prix_Circuit.sid",
    "/MUSICIANS/B/Bjerregaard_Johannes/Nightdawn.sid",
    "/MUSICIANS/F/Follin_Tim/Scumball.sid",
    "/MUSICIANS/B/Beben_Wally/R_I_S_K.sid",
    "/MUSICIANS/F/Future_Freak/Rocky_Star.sid",
    "/MUSICIANS/H/Hubbard_Rob/Mega_Apocalypse.sid",
    "/MUSICIANS/H/Huelsbeck_Chris/Katakis.sid",
    "/MUSICIANS/H/Hubbard_Rob/Gerry_the_Germ.sid",
    "/MUSICIANS/A/ATOO/Compleeto.sid",
    "/MUSICIANS/T/Tel_Jeroen/Eliminator.sid",
    "/MUSICIANS/D/Daglish_Ben/Bulldog.sid",
    "/MUSICIANS/C/Cooksey_Mark/Ramparts.sid",
    "/MUSICIANS/D/Daglish_Ben/Ark_Pandora.sid",
    "/MUSICIANS/H/Hubbard_Rob/Warhawk.sid",
    "/MUSICIANS/D/Daglish_Ben/Vikings.sid",
    "/MUSICIANS/F/FAME/Hendriks_Michael/Dynamic_Range.sid",
    "/MUSICIANS/G/Galway_Martin/Times_of_Lore.sid",
    "/MUSICIANS/J/JCH/Batman_long.sid",
    "/MUSICIANS/G/Galway_Martin/Mikie.sid",
    "/MUSICIANS/C/Crowther_Antony/Zig_Zag.sid",
    "/MUSICIANS/T/Tel_Jeroen/Battle_Valley.sid",
    "/MUSICIANS/J/JCH/Chordian.sid",
    "/MUSICIANS/T/Tel_Jeroen/Combat_Crazy.sid",
    "/MUSICIANS/T/Tel_Jeroen/Hawkeye.sid",
    "/MUSICIANS/G/Galway_Martin/Short_Circuit.sid",
    "/MUSICIANS/L/Laxity/DNA_Warrior.sid",
    "/MUSICIANS/W/Whittaker_David/Glider_Rider.sid",
    "/MUSICIANS/T/Tel_Jeroen/Kinetix.sid",
    "/MUSICIANS/T/Turner_Steve/Uridium.sid",
    "/MUSICIANS/B/Bjerregaard_Johannes/Thunder_Force.sid",
    "/MUSICIANS/H/Hubbard_Rob/Kentilla.sid",
    "/MUSICIANS/B/Bjerregaard_Johannes/Crazy_Comets_remix.sid",
    "/MUSICIANS/C/Clarke_Peter/Mission_of_Mercy.sid",
    "/MUSICIANS/T/Tel_Jeroen/Afterburner.sid",
    "/MUSICIANS/B/Baldwin_Neil/Another_Tune_for_Joanne.sid",
    "/MUSICIANS/G/Galway_Martin/Neverending_Story.sid",
    "/MUSICIANS/G/Galway_Martin/Ocean_Loader_2.sid",
    "/MUSICIANS/W/Walker_Martin/Snare.sid",
    "/MUSICIANS/B/Brennan_Neil/Fist_II-The_Legend_Continues.sid",
];

/// Path of the managed playlist inside the playlist library.
pub fn playlist_path() -> PathBuf {
    playlist::playlists_dir().join(format!("{PLAYLIST_NAME}.m3u"))
}

/// Builds the playlist from the curated list, prefixing each HVSC path with
/// the selected mirror location.
pub fn build_playlist(mirror_location: &str) -> Playlist {
    let mut result = Playlist::empty();
    for &path in FAVOURITE_TOP_100 {
        result.add(&mirror_resource_url(mirror_location, path), None);
    }
    result
}

/// The exact M3U text generated from the list without intermediate Playlist allocations.
fn m3u_content(mirror_location: &str) -> String {
    // Ortalama girdi uzunluğu: URL boyutu + ~45 karakter yol + 1 yeni satır
    let estimated_entry_len = mirror_location.len() + 45;
    let mut content = String::with_capacity(FAVOURITE_TOP_100.len() * estimated_entry_len);
    for &path in FAVOURITE_TOP_100 {
        content.push_str(&mirror_resource_url(mirror_location, path));
        content.push('\n');
    }
    content
}

/// Creates the M3U file when enabled, and removes it when disabled.
///
/// Existing content that already matches the list is left untouched, so an
/// app launch does not rewrite the file unnecessarily. The mirror location is
/// part of the comparison, so switching mirrors regenerates the playlist.
pub fn sync(enabled: bool, mirror_location: &str) -> io::Result<()> {
    let path = playlist_path();
    if !enabled {
        return match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
    }

    let content = m3u_content(mirror_location);
    if fs::read_to_string(&path).ok().as_deref() == Some(content.as_str()) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    build_playlist(mirror_location).save(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favourite_entry_urls_follow_the_selected_mirror() {
        let playlist = build_playlist("https://hvsc.sannic.nl/?dir=C64Music");
        assert_eq!(
            playlist.entries[0].source,
            "https://hvsc.sannic.nl/C64Music/MUSICIANS/D/Daglish_Ben/Last_Ninja.sid"
        );
        assert!(playlist.entries[0].is_url());
    }
}
