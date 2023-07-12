// Using events to drive Firestore operations

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_firebase_auth::{log_in, log_out, GotAuthUrl};
use bevy_firebase_firestore::{
    value::ValueType, CreateDocumentEvent, CreateDocumentResponseEvent, DeleteDocumentEvent,
    DeleteDocumentResponseEvent, FirestoreState, ReadDocumentEvent, ReadDocumentResponseEvent,
    UpdateDocumentEvent, UpdateDocumentResponseEvent, Value,
};

#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
enum AppAuthState {
    #[default]
    Setup,
    LogIn,
    LogOut,
}

// Track progress
#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
enum CrudProgress {
    #[default]
    Setup,
    Create,
    Read1,
    Update,
    Read2,
    Delete,
    Read3,
}

fn main() {
    App::new()
        // Plugins
        .add_plugins(DefaultPlugins)
        .add_plugin(bevy_firebase_auth::AuthPlugin {
            firebase_project_id: "test-auth-rs".into(),
            ..Default::default()
        })
        .add_plugin(bevy_firebase_firestore::FirestorePlugin {
            emulator_url: Some("http://127.0.0.1:8080".into()),
        })
        .add_plugin(bevy_tokio_tasks::TokioTasksPlugin::default())
        // Auth
        .add_state::<AppAuthState>()
        .add_system(auth_url_listener)
        .add_system(log_in.in_schedule(OnEnter(AppAuthState::LogIn)))
        .add_system(log_out.in_schedule(OnEnter(AppAuthState::LogOut)))
        // Test fns
        .add_state::<CrudProgress>()
        .add_system(input)
        .add_system(firestore_ready.in_schedule(OnEnter(FirestoreState::Ready)))
        .add_system(create_test_document.in_schedule(OnEnter(CrudProgress::Create)))
        .add_system(read_test_document.in_schedule(OnEnter(CrudProgress::Read1)))
        .add_system(update_test_document.in_schedule(OnEnter(CrudProgress::Update)))
        .add_system(read_test_document.in_schedule(OnEnter(CrudProgress::Read2)))
        .add_system(delete_test_document.in_schedule(OnEnter(CrudProgress::Delete)))
        .add_system(read_test_document.in_schedule(OnEnter(CrudProgress::Read3)))
        // Response handlers
        .add_system(create_document_response_event_handler)
        .add_system(read_document_response_event_handler)
        .add_system(update_document_response_event_handler)
        .add_system(delete_document_response_event_handler)
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

fn create_document_response_event_handler(
    mut er: EventReader<CreateDocumentResponseEvent>,
    mut next_state: ResMut<NextState<CrudProgress>>,
) {
    for e in er.iter() {
        match e.result.clone() {
            Ok(result) => {
                println!("Document created: {:?}", result);
                next_state.set(CrudProgress::Read1)
            }
            Err(status) => {
                println!("ERROR: Document create failed: {}", status);
                next_state.set(CrudProgress::Read1)
            }
        }
    }
}

fn read_document_response_event_handler(
    mut er: EventReader<ReadDocumentResponseEvent>,
    mut next_state: ResMut<NextState<CrudProgress>>,
    current_state: Res<State<CrudProgress>>,
) {
    for e in er.iter() {
        match e.result.clone() {
            Ok(result) => {
                println!("Document read: {:?}", result);
                match current_state.0 {
                    CrudProgress::Read1 => next_state.set(CrudProgress::Update),
                    CrudProgress::Read2 => next_state.set(CrudProgress::Delete),
                    CrudProgress::Read3 => (),
                    _ => panic!("state machine broke"),
                }
            }
            Err(status) => {
                println!("ERROR: Document read failed: {}", status)
            }
        }
    }
}

fn update_document_response_event_handler(
    mut er: EventReader<UpdateDocumentResponseEvent>,
    mut next_state: ResMut<NextState<CrudProgress>>,
) {
    for e in er.iter() {
        match e.result.clone() {
            Ok(result) => {
                println!("Document updated: {:?}", result);
                next_state.set(CrudProgress::Read2)
            }
            Err(status) => {
                println!("ERROR: Document update failed: {}", status)
            }
        }
    }
}

fn delete_document_response_event_handler(
    mut er: EventReader<DeleteDocumentResponseEvent>,
    mut next_state: ResMut<NextState<CrudProgress>>,
) {
    for e in er.iter() {
        match e.result.clone() {
            Ok(result) => {
                println!("Document deleted: {:?}", result);
                next_state.set(CrudProgress::Read3)
            }
            Err(status) => {
                println!("ERROR: Document delete failed: {}", status)
            }
        }
    }
}

fn firestore_ready(mut next_state: ResMut<NextState<CrudProgress>>) {
    // Start operations when firestore is ready
    next_state.set(CrudProgress::Create);
}

fn create_test_document(mut document_creator: EventWriter<CreateDocumentEvent>) {
    let document_id = "test_document".to_owned();

    let mut document_data = HashMap::new();

    document_data.insert(
        "test_field".to_string(),
        Value {
            value_type: Some(ValueType::IntegerValue(69)),
        },
    );

    document_creator.send(CreateDocumentEvent {
        document_id,
        collection_id: "test_collection".into(),
        document_data,
        id: 0,
    });
}

fn read_test_document(mut document_reader: EventWriter<ReadDocumentEvent>) {
    let document_path = "test_collection/test_document".into();
    document_reader.send(ReadDocumentEvent {
        document_path,
        id: 1,
    })
}

fn update_test_document(mut document_updater: EventWriter<UpdateDocumentEvent>) {
    let document_path = "test_collection/test_document".into();
    let mut document_data = HashMap::new();

    document_data.insert(
        "test_field".to_string(),
        Value {
            value_type: Some(ValueType::IntegerValue(420)),
        },
    );

    document_updater.send(UpdateDocumentEvent {
        document_path,
        document_data,
        id: 2,
    })
}

fn delete_test_document(mut document_deleter: EventWriter<DeleteDocumentEvent>) {
    let document_path = "test_collection/test_document".into();
    document_deleter.send(DeleteDocumentEvent {
        document_path,
        id: 3,
    })
}
