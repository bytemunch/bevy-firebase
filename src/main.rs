use std::collections::HashMap;

use bevy::prelude::*;
use bevy_firebase::{
    deps::{Status, Value, ValueType},
    log_in, log_out, AuthState,
    {
        add_listener, create_document, delete_document, read_document, update_document,
        BevyFirestoreClient, ListenerEvent,
    },
    {GotAuthUrl, ProjectId, UserId},
};
use bevy_tokio_tasks::TokioTasksRuntime;

#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
enum AppAuthState {
    #[default]
    Setup,
    LogIn,
    LogOut,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugin(bevy_firebase::AuthPlugin {
            firebase_project_id: "test-auth-rs".into(),
            ..Default::default()
        })
        .add_plugin(bevy_firebase::FirestorePlugin {
            emulator_url: Some("http://127.0.0.1:8080".into()),
        })
        .add_plugin(bevy_tokio_tasks::TokioTasksPlugin::default())
        .add_system(input)
        .add_system(test_firestore_operations.in_schedule(OnEnter(AuthState::LoggedIn)))
        .add_system(test_listener_system)
        .add_system(auth_url_listener)
        .add_state::<AppAuthState>()
        .add_system(log_in.in_schedule(OnEnter(AppAuthState::LogIn)))
        .add_system(log_out.in_schedule(OnEnter(AppAuthState::LogOut)))
        .run();
}

fn input(keys: Res<Input<KeyCode>>, mut next_state: ResMut<NextState<AppAuthState>>) {
    if keys.just_pressed(KeyCode::I) {
        next_state.set(AppAuthState::LogIn);
    }

    if keys.just_pressed(KeyCode::O) {
        next_state.set(AppAuthState::LogOut);
    }
}

fn auth_url_listener(mut er: EventReader<GotAuthUrl>) {
    for e in er.iter() {
        println!("Go to this URL to sign in:\n{}\n", e.0);
    }
}

fn test_listener_system(mut er: EventReader<ListenerEvent>) {
    for ev in er.iter() {
        println!("EVENT! {:?}", ev.0);
    }
}

fn test_firestore_operations(
    client: ResMut<BevyFirestoreClient>,
    runtime: ResMut<TokioTasksRuntime>,
    project_id: Res<ProjectId>,
    uid: Res<UserId>,
) {
    let uid = uid.0.clone();
    let project_id = project_id.0.clone();
    let mut client = client.clone();

    let mut data = HashMap::new();

    data.insert(
        "test_field".to_string(),
        Value {
            value_type: Some(ValueType::IntegerValue(69)),
        },
    );

    let document_path = &format!("lobbies/{}", uid);

    add_listener(
        &runtime,
        &mut client,
        project_id.clone(),
        document_path.clone(),
        "test".into(),
    );

    runtime.spawn_background_task(|mut ctx| async move {
        let document_path = &format!("lobbies/{}", uid);

        let _ = create_document(
            &mut client,
            &project_id,
            &uid,
            &"lobbies".into(),
            data.clone(),
        )
        .await;

        let read = read_document(&mut client, &project_id, document_path).await;
        println!("READ 1: {:?}\n", read);

        data.insert(
            "test_field".into(),
            Value {
                value_type: Some(ValueType::IntegerValue(420)),
            },
        );

        ctx.sleep_updates(30).await;

        let _ = update_document(&mut client, &project_id, document_path, data.clone()).await;

        let read = read_document(&mut client, &project_id, document_path).await;
        println!("READ 2: {:?}\n", read);

        let _ = delete_document(&mut client, &project_id, document_path).await;

        let read = read_document(&mut client, &project_id, document_path).await;
        println!("READ 3: {:?}\n", read);

        Ok::<(), Status>(())
    });
}
