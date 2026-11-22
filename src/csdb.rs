// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

//! CSDb lookup for the releases that use a SID file from HVSC.
//!
//! The browser page is server-rendered, so the same table the original
//! chromiumoxide-based helper read can be fetched and parsed with `ureq` and
//! `scraper` without starting a second browser inside SidCraft.

use scraper::{ElementRef, Html, Selector, node::Node};
use std::io::Read;

/// Base address used for every CSDb link shown in the dialog.
pub const CSDB_BASE: &str = "https://csdb.dk";

/// Search endpoint that resolves an HVSC path to its CSDb SID entry.
const SEARCH_URL: &str = "https://csdb.dk/search/?seinsel=sids&search=";

/// One row of CSDb's "Releases which use this SID" table.
///
/// The fields match the five CSV columns of the original helper: release link,
/// release name, group/scener link, group/scener name and the trailing info.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidRelease {
    pub release_path: String,
    pub release: String,
    pub author_path: String,
    pub author: String,
    pub info: String,
}

/// Full CSDb URL for a relative link returned by [`fetch_releases`].
pub fn url_for(path: &str) -> String {
    let path = path.trim();
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else if path.starts_with('/') {
        format!("{CSDB_BASE}{path}")
    } else {
        format!("{CSDB_BASE}/{path}")
    }
}

/// Fetches the CSDb page for `sid_path` and parses its release table.
pub fn fetch_releases(sid_path: &str) -> Result<Vec<SidRelease>, String> {
    let url = format!("{SEARCH_URL}{}", encode_search_value(sid_path));
    let response = ureq::get(&url)
        .call()
        .map_err(|e| format!("CSDb request failed: {e}"))?;
    let mut html = String::new();
    response
        .into_body()
        .into_reader()
        .read_to_string(&mut html)
        .map_err(|e| format!("Could not read CSDb response: {e}"))?;
    parse_releases(&html)
}

/// Keeps path separators readable in the query while escaping other characters.
fn encode_search_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'_' | b'-' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Finds the first table after the "Releases which use this SID" heading.
fn find_releases_table(doc: &Html) -> Option<ElementRef<'_>> {
    let mut found_heading = false;
    for node in doc.root_element().descendants() {
        match node.value() {
            Node::Text(text) if text.contains("Releases which use this SID") => {
                found_heading = true;
            }
            Node::Element(element) if found_heading && element.name() == "table" => {
                return ElementRef::wrap(node);
            }
            _ => {}
        }
    }
    None
}

/// Normalizes a CSDb href to the `/release/?id=…` style used in the result.
fn csdb_suffix(href: &str) -> Option<String> {
    let href = href.trim();
    let path = href
        .strip_prefix("https://csdb.dk")
        .or_else(|| href.strip_prefix("http://csdb.dk"))
        .unwrap_or(href);
    let path = path.split('#').next().unwrap_or(path).to_string();
    if path.starts_with("/release/?id=")
        || path.starts_with("/group/?id=")
        || path.starts_with("/scener/?id=")
    {
        Some(path)
    } else {
        None
    }
}

fn text_of(element: &ElementRef<'_>) -> String {
    element.text().collect::<String>().trim().to_string()
}

/// Parses the release table from a CSDb SID page.
fn parse_releases(html: &str) -> Result<Vec<SidRelease>, String> {
    let doc = Html::parse_document(html);
    let table = find_releases_table(&doc)
        .ok_or_else(|| "'Releases which use this SID' table was not found".to_string())?;
    let tr_selector = Selector::parse("tr").expect("valid tr selector");
    let td_selector = Selector::parse("td").expect("valid td selector");
    let a_selector = Selector::parse("a").expect("valid a selector");

    let mut rows = Vec::new();
    for tr in table.select(&tr_selector) {
        let cells: Vec<ElementRef<'_>> = tr.select(&td_selector).collect();
        let Some(first) = cells.first() else {
            continue;
        };

        let mut release = None;
        let mut author = None;
        for anchor in first.select(&a_selector) {
            let Some(href) = anchor.value().attr("href") else {
                continue;
            };
            let Some(path) = csdb_suffix(href) else {
                continue;
            };
            if release.is_none() && path.starts_with("/release/?id=") {
                release = Some((anchor, path.clone()));
            }
            if author.is_none()
                && (path.starts_with("/group/?id=") || path.starts_with("/scener/?id="))
            {
                author = Some((anchor, path));
            }
        }
        let (Some((release_link, release_path)), Some((author_link, author_path))) =
            (release, author)
        else {
            continue;
        };

        rows.push(SidRelease {
            release_path,
            release: text_of(&release_link),
            author_path,
            author: text_of(&author_link),
            info: cells.get(2).map(text_of).unwrap_or_default(),
        });
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_group_and_scener_rows_in_table_order() {
        let html = r#"
            <b>Releases which use this SID :</b><br>
            <table>
              <tr>
                <td><a href="/release/download.php?id=1">DL</a><a href="/release/?id=265267">Ascraeus Crack Intro</a> by <a href="/group/?id=210"><font color="blue">Ascraeus</font></a></td>
                <td>&nbsp;...&nbsp;</td>
                <td><font size="1">2026 Intro</font></td>
              </tr>
              <tr>
                <td><a href="/release/?id=227290">Robocop 3 Preview</a> by <a href="/scener/?id=1206">PDB</a></td>
                <td>&nbsp;...&nbsp;</td>
                <td>1992 Music</td>
              </tr>
            </table>
        "#;

        let rows = parse_releases(html).unwrap();
        assert_eq!(
            rows,
            vec![
                SidRelease {
                    release_path: "/release/?id=265267".to_string(),
                    release: "Ascraeus Crack Intro".to_string(),
                    author_path: "/group/?id=210".to_string(),
                    author: "Ascraeus".to_string(),
                    info: "2026 Intro".to_string(),
                },
                SidRelease {
                    release_path: "/release/?id=227290".to_string(),
                    release: "Robocop 3 Preview".to_string(),
                    author_path: "/scener/?id=1206".to_string(),
                    author: "PDB".to_string(),
                    info: "1992 Music".to_string(),
                },
            ]
        );
    }

    #[test]
    fn missing_table_is_reported() {
        let error = parse_releases("<p>no table here</p>").unwrap_err();
        assert!(error.contains("Releases which use this SID"));
    }

    #[test]
    fn urls_are_built_from_csdb_relative_links() {
        assert_eq!(
            url_for("/release/?id=265267"),
            "https://csdb.dk/release/?id=265267"
        );
        assert_eq!(
            url_for("/scener/?id=1206"),
            "https://csdb.dk/scener/?id=1206"
        );
    }
}
