use std::collections::HashMap;

use bevy::prelude::*;
use bevy_firebase::{
    auth::{ProjectId, UserId},
    deps::{Status, Value, ValueType},
    firestore::{
        add_listener, create_document, delete_document, read_document, update_document,
        BevyFirestoreClient, ListenerEvent,
    },
};
use bevy_tokio_tasks::TokioTasksRuntime;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugin(bevy_firebase::auth::AuthPlugin {
            firebase_project_id: "test-auth-rs".into(),
            ..Default::default()
        })
        .add_plugin(bevy_firebase::firestore::FirestorePlugin {
            emulator_url: Some("http://127.0.0.1:8080".into()),
        })
        .add_plugin(bevy_tokio_tasks::TokioTasksPlugin::default())
        .add_system(test_firestore_operations)
        .add_system(test_listener_system)
        .run();
}

fn test_listener_system(mut er: EventReader<ListenerEvent>) {
    for ev in er.iter() {
        println!("EVENT! {:?}", ev.0);
    }
}

fn test_firestore_operations(
    client: Option<ResMut<BevyFirestoreClient>>,
    runtime: ResMut<TokioTasksRuntime>,
    project_id: Option<Res<ProjectId>>,
    uid: Option<Res<UserId>>,
) {
    if let (Some(client), Some(project_id), Some(uid)) = (client, project_id, uid) {
        if !client.is_added() {
            return;
        }

        let mut data = HashMap::new();

        data.insert(
            "test_field".to_string(),
            Value {
                value_type: Some(ValueType::IntegerValue(69)),
            },
        );

        let mut client = client.clone();
        let project_id = project_id.0.clone();
        let uid = uid.0.clone();
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
}
