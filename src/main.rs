use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // .add_plugin(LogDiagnosticsPlugin::default())
        // .add_plugin(FrameTimeDiagnosticsPlugin::default())
        .add_plugin(bevy_firebase::auth::AuthPlugin {
            firebase_api_key: "FIREBASE_API_KEY".into(),
            google_client_id:
                "CLIENT_ID_STRING.apps.googleusercontent.com".into(),
            google_client_secret: "GOOGLE_CLIENT_SECRET".into(),
            firebase_refresh_token: None,
            // firebase_refresh_token: Some("REFRESH_TOKEN".into()),
            firebase_project_id: "test-auth-rs".into(),
        })
        .add_plugin(bevy_firebase::firestore::FirestorePlugin)
        .run()
}
