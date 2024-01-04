use async_recursion::async_recursion;
use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use regex::Regex;
use rspotify::model::playlist::PlaylistTracksRef;
use rspotify::model::{
    FullPlaylist, FullTrack, Page, PlayableItem, PlaylistId, PlaylistItem, PublicUser,
    SimplifiedPlaylist, UserId,
};
use rspotify::{
    prelude::*, scopes, AuthCodeSpotify, ClientCredsSpotify, ClientError, Config, Credentials,
    OAuth,
};

// local caching to speed up development
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};

use std::collections::HashSet;
#[derive(Serialize, Deserialize, Clone)]
pub struct PlaylistTrack {
    pub added_at: Option<DateTime<Utc>>,
    pub added_by: Option<PublicUser>,
    pub is_local: bool,
    pub track: Option<FullTrack>,
}

#[derive(Serialize, Deserialize)]
struct Tracks {
    list: Vec<PlaylistTrack>,
}

#[tokio::main]
async fn main() {
    println!("Hello, lets talk with Spotify!");

    // TODO: would be nice to move this to be const (global) but need to figure out how :)
    let recent_onehundred_id: PlaylistId =
        PlaylistId::from_uri("spotify:playlist:5j1TO7kHW28exlT5HaBua1").unwrap();

    let creds = Credentials::from_env().unwrap();
    let oauth = OAuth::from_env(scopes!(
        "playlist-read-collaborative",
        "playlist-modify-public"
    ))
    .unwrap(); // for now I am excluding private playlists "playlist-read-private"

    // let spotify: ClientCredsSpotify = ClientCredsSpotify::new(creds); // no user specific actions available
    // spotify.request_token().await.unwrap();

    let mut config = Config::default();
    config.token_cached = true;

    let spotify = AuthCodeSpotify::with_config(creds, oauth, config);

    // no token caching
    let url = spotify.get_authorize_url(false).unwrap();
    spotify.prompt_for_token(&url).await.unwrap();

    // token caching
    // let token_available = spotify.read_token_cache(false).await;
    // match token_available {
    //     Ok(token) => {
    //         println!("cached token available: {:?}", token.unwrap());
    //         spotify.get_oauth();
    //     },
    //     Err(error) => {
    //         dbg!(error);
    //         let url = spotify.get_authorize_url(false).unwrap();
    //         spotify.prompt_for_token(&url).await.unwrap();
    //     },
    // }

    // collect all user tracks
    let mut all_tracks = get_all_user_tracks(&spotify).await;
    println!("all user tracks total: {}", all_tracks.len());

    // order from newest to oldest
    all_tracks.sort_by(|a, b| b.added_at.unwrap().cmp(&a.added_at.unwrap()));
    println!("first track: {}", all_tracks[0].added_at.unwrap());

    // remove duplicates (same track added to different playlists)
    let mut track_name_seen = std::collections::HashSet::new();
    all_tracks.retain(|track| {
        if let Some(track) = track.track.as_ref() {
            if let Some(track_id) = track.id.as_ref() {
                return track_name_seen.insert(track_id.clone());
            }
        }

        // some helpful debug info surrounding troublesome tracks
        // if let Some(track_track) = &track.track {
        //     dbg!(track_track);
        // } else {
        //     dbg!(&track.is_local);
        // }

        // we remove any tracks for which we cannot retrive an ID
        false
    });
    println!("total unique (valid) user tracks: {}", all_tracks.len());

    // most recent 100 tracks
    let mut recent_onehundred: Vec<PlaylistTrack> = all_tracks
        .iter()
        .take(100)
        .map(|track| track.clone())
        .collect();

    // print these track names
    // for track in &recent_onehundred {
    //     println!("{}", track.track.as_ref().unwrap().name);
    // }

    let current_recent_onehundred_tracks =
        get_tracks_with_playlist_id(&spotify, &recent_onehundred_id).await;
    let mut tracks_to_remove = current_recent_onehundred_tracks.clone();
    let mut tracks_to_add = recent_onehundred.clone();

    remove_tracks(&mut tracks_to_remove, &recent_onehundred); // remaining in tracks_to_remove are the tracks we must ask spotify to remove from the recent 100 playlist
    remove_tracks(&mut tracks_to_add, &current_recent_onehundred_tracks); // remaining in recent_onehundred are the tracks we must ask spotify to add to the recent 100 playlist

    println!("tracks we must remove: {}", tracks_to_remove.len());
    println!("tracks we must add: {}", tracks_to_add.len());

    add_to_playlist(&spotify, &recent_onehundred_id, &tracks_to_add).await;
    remove_from_playlist(&spotify, &recent_onehundred_id, &tracks_to_remove).await;
}

async fn user_playlists(spotify: &AuthCodeSpotify) -> Vec<SimplifiedPlaylist> {
    let user_playlists: std::pin::Pin<
        Box<
            dyn Stream<Item = Result<rspotify::model::SimplifiedPlaylist, rspotify::ClientError>>
                + Send,
        >,
    > = spotify.user_playlists(UserId::from_id("8yph5tkc63geq87roiesbqa36").unwrap());

    let mut playlist_stream = Box::pin(user_playlists);

    let mut user_playlists: Vec<SimplifiedPlaylist> = Vec::new();
    while let Some(result) = playlist_stream.next().await {
        match result {
            Ok(playlist) => {
                user_playlists.push(playlist);
            }
            Err(error) => {
                eprintln!("We got an error! {}", error);
            }
        }
    }
    return user_playlists;
}

#[async_recursion]
async fn get_all_playable_tracks(
    spotify: &AuthCodeSpotify,
    tracks: &Page<PlaylistItem>,
) -> Vec<PlaylistTrack> {
    let mut playlist_tracks: Vec<PlaylistTrack> = tracks
        .items
        .iter()
        .map(|playlist_item| PlaylistTrack {
            added_at: playlist_item.added_at.clone(),
            added_by: playlist_item.added_by.clone(),
            is_local: playlist_item.is_local,
            track: if let Some(playable_item) = &playlist_item.track {
                match playable_item {
                    PlayableItem::Track(track) => Some(track.clone()),
                    PlayableItem::Episode(_) => None,
                }
            } else {
                None
            },
        })
        .collect();

    if let Some(next_href) = &tracks.next {
        println!("recursing to get next page!");

        let re_capture_id = Regex::new(r"/playlists/([a-zA-Z0-9]+)").unwrap();
        let re_capture_offset = Regex::new(r"offset=([0-9]+)").unwrap();
        let re_capture_limit = Regex::new(r"limit=([0-9]+)").unwrap();

        let next_id = re_capture_id
            .captures(&next_href)
            .unwrap()
            .get(1)
            .unwrap()
            .as_str();
        let next_offset = re_capture_offset
            .captures(&next_href)
            .unwrap()
            .get(1)
            .unwrap()
            .as_str()
            .parse::<u32>()
            .unwrap();
        let next_limit = re_capture_limit
            .captures(&next_href)
            .unwrap()
            .get(1)
            .unwrap()
            .as_str()
            .parse::<u32>()
            .unwrap();

        playlist_tracks.extend(
            get_all_playable_tracks(
                &spotify,
                &spotify
                    .playlist_items_manual(
                        PlaylistId::from_id(next_id).unwrap(),
                        None,
                        None,
                        Some(next_limit),
                        Some(next_offset),
                    )
                    .await
                    .unwrap(),
            )
            .await
            .clone(),
        );
        return playlist_tracks;
    } else {
        println!("no next page!");
    }

    return playlist_tracks;
}

async fn get_tracks(
    spotify: &AuthCodeSpotify,
    playlist: &SimplifiedPlaylist,
) -> Vec<PlaylistTrack> {
    // TODO: could filter for only track field let track_list = playlist.tracks;
    if let Ok(playlist) = spotify.playlist(playlist.id.clone(), None, None).await {
        // TODO: need to check all pages, if there are more than 1 pages.
        // generate a vector of playlist tracks using a custom 'closure'
        return get_all_playable_tracks(&spotify, &playlist.tracks).await;
    } else {
        return Vec::<PlaylistTrack>::new();
    }
}

async fn get_tracks_with_playlist_id(
    spotify: &AuthCodeSpotify,
    playlist_id: &PlaylistId<'_>,
) -> Vec<PlaylistTrack> {
    let playlist = spotify.playlist(playlist_id.clone(), None, None).await;
    let playlist = match playlist {
        Ok(playlist) => playlist,
        Err(err) => {
            dbg!(err);
            return Vec::<PlaylistTrack>::new();
        }
    };

    return get_all_playable_tracks(&spotify, &playlist.tracks).await;
}

async fn get_all_user_tracks(spotify: &AuthCodeSpotify) -> Vec<PlaylistTrack> {
    let mut all_user_tracks: Vec<PlaylistTrack> = Vec::new();
    #[cfg(not(feature = "use_cached_results"))]
    {
        //retrieve all playlists
        let playlists = user_playlists(&spotify).await;

        // debug - print all playlists
        for playlist in &playlists {
            println!(
                "Playlist name: {}, Track count: {}, Playlist ID: {},  snapshot_id: {}",
                playlist.name, playlist.tracks.total, playlist.id, playlist.snapshot_id
            );
        }

        // build a vector containing all tracks - EVER!
        // we loose information regarding which playlist a track belongs here!
        for playlist in &playlists {
            println!("collecting all tracks from {}", playlist.name);
            all_user_tracks.extend(get_tracks(&spotify, &playlist).await);
        }

        #[cfg(feature = "cache_new_results")]
        {
            let all_tracks = Tracks {
                list: all_user_tracks,
            };

            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open("cached_all_tracks.json")
                .unwrap();
            serde_json::to_writer(file, &all_tracks).unwrap();
            all_user_tracks = all_tracks.list;
            println!("all tracks have been cached to disk");
        }
    }

    #[cfg(feature = "use_cached_results")]
    {
        println!("offline: using cached track list from disk!");
        let file = File::open("cached_all_tracks.json").unwrap();
        let all_tracks: Tracks = serde_json::from_reader(file).unwrap();
        all_user_tracks = all_tracks.list;
        println!("cached tracks loaded!");
    }

    return all_user_tracks;
}

fn remove_tracks(tracks: &mut Vec<PlaylistTrack>, removal_list: &Vec<PlaylistTrack>) {
    println!(
        "tracks len: {} removal list len: {}",
        tracks.len(),
        removal_list.len()
    );
    tracks.retain(|track| {
        !removal_list.iter().any(|removal_track| {
            track.track.as_ref().unwrap().id.as_ref().unwrap()
                == removal_track.track.as_ref().unwrap().id.as_ref().unwrap()
        })
    });
}

// the remove_from_playlist and add_to_playlist are technically the same logic with different api call - TODO: combine functionality
async fn remove_from_playlist(
    spotify: &AuthCodeSpotify,
    playlist_id: &PlaylistId<'_>,
    tracks: &Vec<PlaylistTrack>,
) {
    if tracks.len() == 0 {
        return;
    }

    let track_ids: Vec<rspotify::model::PlayableId> = tracks
        .iter()
        .map(|track| PlayableId::Track(track.track.as_ref().unwrap().id.as_ref().unwrap().clone()))
        .collect();

    match spotify
        .playlist_remove_all_occurrences_of_items(playlist_id.clone(), track_ids, None)
        .await
    {
        Ok(result) => {
            println!("tracks removed from playlist successfully!");
        }
        Err(error) => {
            dbg!(error);
        }
    }
}

async fn add_to_playlist(
    spotify: &AuthCodeSpotify,
    playlist_id: &PlaylistId<'_>,
    tracks: &Vec<PlaylistTrack>,
) {
    if tracks.len() == 0 {
        return;
    }

    let track_ids: Vec<rspotify::model::PlayableId> = tracks
        .iter()
        .map(|track| PlayableId::Track(track.track.as_ref().unwrap().id.as_ref().unwrap().clone()))
        .collect();

    match spotify
        .playlist_add_items(playlist_id.clone(), track_ids, None)
        .await
    {
        Ok(result) => {
            println!("tracks added to playlist successfully!");
        }
        Err(error) => {
            dbg!(error);
        }
    }
}
