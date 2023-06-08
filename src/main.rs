use bevy::prelude::*;
use bevy_firebase::echo_test;

fn main() {
    echo_test("bananana".into());

    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugin(bevy_firebase::bevy::BevyFirebasePlugin {
            firebase_api_key: "FIREBASE_API_KEY".into(),
            google_client_id: "".into(),
            google_client_secret: "".into(),
            firebase_refresh_token: Some("REFRESH_TOKEN".into()),
        })
        .run()
}
