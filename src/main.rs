use bevy::{prelude::*, diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin}};
use bevy_firebase::echo_test;

fn main() {
    echo_test("bananana".into());

    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugin(LogDiagnosticsPlugin::default())
        .add_plugin(FrameTimeDiagnosticsPlugin::default())
        .add_plugin(bevy_firebase::bevy::BevyFirebasePlugin {
            firebase_api_key: "FIREBASE_API_KEY".into(),
            google_client_id: "CLIENT_ID_STRING.apps.googleusercontent.com".into(),
            google_client_secret: "GOOGLE_CLIENT_SECRET".into(),
            firebase_refresh_token: None,
            // firebase_refresh_token: Some("REFRESH_TOKEN".into()),
            firebase_project_id: "test-auth-rs".into()
        })
        // testing blocking
        .add_startup_system(setup)
        .add_system(second_counter)
        .run()
}

#[derive(Resource)]
struct SecondCounter(Timer);

fn setup(mut commands:Commands) {
    commands.insert_resource(SecondCounter(Timer::from_seconds(0.5, TimerMode::Repeating)));
}

fn second_counter(mut timer:ResMut<SecondCounter>, time: Res<Time>) {
    let delta = time.delta();
    timer.0.tick(delta);
    if timer.0.just_finished() {
        println!("TIMER!");
    }
}