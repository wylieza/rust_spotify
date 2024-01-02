use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use rspotify::model::playlist::PlaylistTracksRef;
use rspotify::model::FullTrack;
use rspotify::model::PlayableItem;
use rspotify::model::PlaylistId;
use rspotify::model::PublicUser;
use rspotify::model::SimplifiedPlaylist;
use rspotify::model::UserId;
use rspotify::{
    // model::{Country, Market, SearchType},
    prelude::*,
    ClientCredsSpotify,
    Credentials,
};

// local caching to speed up development
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};

#[derive(Serialize, Deserialize)]
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

    let creds = Credentials::from_env().unwrap();
    let spotify: ClientCredsSpotify = ClientCredsSpotify::new(creds);

    spotify.request_token().await.unwrap();

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
                .open("cached_all_tracks.json").unwrap();
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

    println!("all user tracks total: {}", all_user_tracks.len());

    // get_tracks(&spotify, &playlists[0]).await;
}

async fn user_playlists(spotify: &ClientCredsSpotify) -> Vec<SimplifiedPlaylist> {
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

async fn get_tracks(
    spotify: &ClientCredsSpotify,
    playlist: &SimplifiedPlaylist,
) -> Vec<PlaylistTrack> {
    let playlist = spotify
        .playlist(playlist.id.clone(), None, None)
        .await
        .unwrap(); // TODO: could filter for only track field
                   // let track_list = playlist.tracks;

    let playlist_tracks: Vec<PlaylistTrack> = playlist
        .tracks
        .items
        .iter()
        .map(|track_item| PlaylistTrack {
            added_at: track_item.added_at.clone(),
            added_by: track_item.added_by.clone(),
            is_local: track_item.is_local,
            track: if let Some(track) = &track_item.track {
                match track {
                    PlayableItem::Track(track) => Some(track.clone()),
                    PlayableItem::Episode(_) => None,
                }
            } else {
                None
            },
        })
        .collect();

    // println!(
    //     "track \"{}\" was added {}",
    //     playlist_tracks.first().unwrap().track.as_ref().unwrap().name,
    //     playlist_tracks.first().unwrap().added_at.unwrap()
    // );

    // println!("track: {}, added: {}", match track_list.items.first().unwrap().track.as_ref().unwrap() {PlayableItem::Track(track) => track.name.clone(), _ => panic!("unexpected type"),}, track_list.items.first().unwrap().added_at.unwrap());
    return playlist_tracks;
}
