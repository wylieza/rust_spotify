use rspotify::model::PlaylistId;
use rspotify::model::UserId;
use rspotify::{
    model::{Country, Market, SearchType},
    prelude::*,
    ClientCredsSpotify, Credentials,
};

#[tokio::main]
async fn main() {
    println!("Hello, lets talk with Spotify!");

    let creds = Credentials::from_env().unwrap();
    let spotify: ClientCredsSpotify = ClientCredsSpotify::new(creds);

    spotify.request_token().await.unwrap();

    let user_id = UserId::from_id("8yph5tkc63geq87roiesbqa36").unwrap();
    let playlist_id = Some(PlaylistId::from_id("1MyAiLaO98MiC1lRet5lbs").unwrap());

    let playlist = spotify
        .user_playlist(
            user_id,     // User ID
            playlist_id, // Playlist ID (without query parameters)
            None,
        )
        .await
        .unwrap();

    // println!("{:#?}", playlist);
    println!("Playlist name: {}", playlist.name);
    println!("Total tracks: {}", playlist.tracks.total);


}
