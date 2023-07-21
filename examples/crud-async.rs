// Performing Firestore operations with await syntax

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_firebase_auth::{log_in, log_out, GotAuthUrl, ProjectId};
use bevy_firebase_firestore::{
    async_create_document,
    value::ValueType,
    FirestoreState, Status, Value,
    {async_delete_document, async_read_document, async_update_document, BevyFirestoreClient},
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
        // Plugins
        .add_plugins(DefaultPlugins)
        .add_plugins(bevy_firebase_auth::AuthPlugin::default())
        .add_plugins(bevy_firebase_firestore::FirestorePlugin::default())
        .add_plugins(bevy_tokio_tasks::TokioTasksPlugin::default())
        // Auth
        .add_state::<AppAuthState>()
        .add_systems(Update, auth_url_listener)
        .add_systems(OnEnter(AppAuthState::LogIn), log_in)
        .add_systems(OnEnter(AppAuthState::LogOut), log_out)
        // Test fns
        .add_systems(Update, input)
        .add_systems(OnEnter(FirestoreState::Ready), async_operations)
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

fn async_operations(
    client: ResMut<BevyFirestoreClient>,
    runtime: ResMut<TokioTasksRuntime>,
    project_id: Res<ProjectId>,
) {
    // Performs document operations one after another

    let collection_id = "test_collection".to_owned();
    let document_id = "test_document".to_owned();
    let project_id = project_id.0.clone();
    let mut client = client.0.clone();

    let mut fields = HashMap::new();

    fields.insert(
        "test_field".to_string(),
        Value {
            value_type: Some(ValueType::IntegerValue(69)),
        },
    );

    runtime.spawn_background_task(|mut _ctx| async move {
        let document_path = &format!("{collection_id}/{document_id}");

        let _ = async_create_document(
            &mut client,
            &project_id,
            &document_id,
            &collection_id,
            fields.clone(),
        )
        .await;

        let read = async_read_document(&mut client, &project_id, document_path).await;
        println!("READ 1: {:?}\n", read);

        // Modify and add data
        fields.insert(
            "test_field".into(),
            Value {
                value_type: Some(ValueType::IntegerValue(420)),
            },
        );

        fields.insert(
            "another_field".into(),
            Value {
                value_type: Some(ValueType::StringValue("Another String".into())),
            },
        );

        let _ =
            async_update_document(&mut client, &project_id, document_path, fields.clone()).await;

        let read = async_read_document(&mut client, &project_id, document_path).await;
        println!("READ 2: {:?}\n", read);

        let _ = async_delete_document(&mut client, &project_id, document_path).await;

        let read = async_read_document(&mut client, &project_id, document_path).await;
        println!("READ 3: {:?}\n", read);

        Ok::<(), Status>(())
    });
}
