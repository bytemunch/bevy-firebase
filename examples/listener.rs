// Adding and using a Firestore listener
use std::collections::HashMap;

use bevy::prelude::*;
use bevy_firebase_auth::{log_in, log_out, GotAuthUrl};
use bevy_firebase_firestore::{
    listen_response::ResponseType, value::ValueType, CreateDocumentEvent, CreateListenerEvent,
    FirestoreState, ListenerResponseEvent, Value,
};

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
        .add_plugins(bevy_firebase_auth::AuthPlugin {
            firebase_project_id: "demo-bevy".into(),
            ..Default::default()
        })
        .add_plugins(bevy_firebase_firestore::FirestorePlugin {
            emulator_url: Some("http://127.0.0.1:8080".into()),
        })
        .add_plugins(bevy_tokio_tasks::TokioTasksPlugin::default())
        // Auth
        .add_state::<AppAuthState>()
        .add_systems(Update, auth_url_listener)
        .add_systems(OnEnter(AppAuthState::LogIn), log_in)
        .add_systems(OnEnter(AppAuthState::LogOut), log_out)
        // Test fns
        .add_systems(Update, input)
        .add_systems(OnEnter(FirestoreState::Ready), create_listener)
        .add_systems(Update, listener_event_handler)
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

fn listener_event_handler(mut er: EventReader<ListenerResponseEvent>) {
    for ev in er.iter() {
        match ev.msg.response_type.as_ref().unwrap() {
            ResponseType::DocumentChange(response) => {
                println!("Document Changed: {:?}", response.document.clone().unwrap());
            }
            ResponseType::DocumentDelete(response) => {
                println!("Document Deleted: {:?}", response.document.clone());
            }
            ResponseType::DocumentRemove(response) => {
                println!("Document Removed: {:?}", response.document.clone());
            }
            _ => {}
        }
    }
}

fn create_listener(
    mut document_creator: EventWriter<CreateDocumentEvent>,
    mut listener_creator: EventWriter<CreateListenerEvent>,
) {
    let mut data = HashMap::new();

    data.insert(
        "test_field".to_string(),
        Value {
            value_type: Some(ValueType::IntegerValue(69)),
        },
    );

    let doc_id = "listener_test";

    let document_path = format!("test_collection/{}", doc_id);

    listener_creator.send(CreateListenerEvent {
        target: document_path,
    });

    document_creator.send(CreateDocumentEvent {
        document_id: doc_id.into(),
        collection_id: "test_collection".into(),
        document_data: data.clone(),
        id: 1337,
    });
}
