use std::collections::HashMap;

use bevy::prelude::*;
use bevy_firebase::{
    auth::{ProjectId, UserId},
    deps::{Status, Value, ValueType},
    firestore::{
        add_listener, create_document, delete_document, read_document, update_document,
        BevyFirestoreClient, MyTestEvent,
    },
};
use bevy_tokio_tasks::TokioTasksRuntime;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // .add_plugin(LogDiagnosticsPlugin::default())
        // .add_plugin(FrameTimeDiagnosticsPlugin::default())
        // TODO add plugin dependencies here
        // .add_plugin(TokioTasksPlugin::default())
        // .add_plugin(pecs plugin)
        .add_plugin(bevy_firebase::auth::AuthPlugin {
            firebase_api_key: "FIREBASE_API_KEY".into(),
            google_client_id:
                "CLIENT_ID_STRING.apps.googleusercontent.com".into(),
            google_client_secret: "GOOGLE_CLIENT_SECRET".into(),
            // firebase_refresh_token: None,
            // TODO refresh token stored to file
            firebase_refresh_token: Some("REFRESH_TOKEN".into()),
            firebase_project_id: "test-auth-rs".into(),
        })
        .add_plugin(bevy_firebase::firestore::FirestorePlugin {
            // emulator_url: None
            emulator_url: Some("http://127.0.0.1:8080".into())
        })
        .add_system(test_firestore_operations)
        .add_system(test_listener_system)
        .run();
}

fn test_listener_system(mut er: EventReader<MyTestEvent>) {
    for ev in er.iter() {
        println!("EVENT! {:?}", ev.0.msg);
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
        );

        runtime.spawn_background_task(|mut ctx| async move {
            let document_path = &format!("lobbies/{}", uid);

            create_document(
                &mut client,
                &project_id,
                &uid,
                &"lobbies".into(),
                data.clone(),
            )
            .await?;
            // TODO fails silently

            let read = read_document(&mut client, &project_id, document_path).await;
            println!("READ 1: {:?}\n", read);

            data.insert(
                "test_field".into(),
                Value {
                    value_type: Some(ValueType::IntegerValue(420)),
                },
            );

            ctx.sleep_updates(30).await;

            update_document(&mut client, &project_id, document_path, data.clone()).await?;

            let read = read_document(&mut client, &project_id, document_path).await;
            println!("READ 2: {:?}\n", read);

            delete_document(&mut client, &project_id, document_path).await?;

            let read = read_document(&mut client, &project_id, document_path).await;
            println!("READ 3: {:?}\n", read);

            Ok::<(), Status>(())
        });
    }
}
