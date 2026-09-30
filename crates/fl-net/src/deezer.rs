//! `DeezerClient`: the open `/search/artist` endpoint, for artist pictures.

use fl_core::ArtistResolver;
use serde::Deserialize;

use crate::{agent, NetError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeezerArtist {
    pub id: i64,
    pub name: String,
    /// `picture_xl`, else `picture_big`, else `picture_medium`.
    pub picture_url: Option<String>,
}

#[derive(Deserialize)]
struct SearchResponse {
    data: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
    id: i64,
    name: String,
    picture_medium: Option<String>,
    picture_big: Option<String>,
    picture_xl: Option<String>,
}

/// The first hit whose folded name matches the query's, else the top hit.
fn pick(hits: Vec<Hit>, query: &str) -> Option<DeezerArtist> {
    let needle = ArtistResolver::key(query);
    let i = hits.iter().position(|h| ArtistResolver::key(&h.name) == needle).unwrap_or(0);
    let h = hits.into_iter().nth(i)?;
    Some(DeezerArtist {
        id: h.id,
        name: h.name,
        picture_url: h.picture_xl.or(h.picture_big).or(h.picture_medium).filter(|s| !s.is_empty()),
    })
}

pub fn search_artist(name: &str) -> Result<Option<DeezerArtist>, NetError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(NetError::InvalidQuery);
    }
    let mut resp = agent()
        .get("https://api.deezer.com/search/artist")
        .query("q", trimmed)
        .query("limit", "5")
        .call()?;
    let body: SearchResponse = resp.body_mut().read_json()?;
    Ok(pick(body.data, trimmed))
}

pub fn download_image(url: &str) -> Result<Vec<u8>, NetError> {
    let mut resp = agent().get(url).call()?;
    Ok(resp.body_mut().with_config().limit(20 * 1024 * 1024).read_to_vec()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: i64, name: &str, xl: Option<&str>) -> Hit {
        Hit { id, name: name.into(), picture_medium: Some("m".into()), picture_big: None, picture_xl: xl.map(Into::into) }
    }

    #[test]
    fn prefers_folded_name_match_then_top_hit() {
        let a = pick(vec![hit(1, "Bjork Tribute", None), hit(2, "Björk", Some("xl"))], "bjork").unwrap();
        assert_eq!((a.id, a.picture_url.as_deref()), (2, Some("xl")));
        let b = pick(vec![hit(7, "Someone Else", None)], "Nobody").unwrap();
        assert_eq!((b.id, b.picture_url.as_deref()), (7, Some("m")));
        assert!(pick(vec![], "x").is_none());
    }
}
