// Using custom event structs with Firestore
use std::collections::HashMap;

use bevy::prelude::*;
use bevy_firebase_auth::{log_in, log_out, GotAuthUrl};
use bevy_firebase_firestore::{
    create_document_event_handler, value::ValueType, CreateDocumentEventBuilder,
    CreateDocumentResponseEventBuilder, DocumentResult, FirestoreState, Value,
};

#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
enum AppAuthState {
    #[default]
    Setup,
    LogIn,
    LogOut,
}

// Custom event structs + impls
#[derive(Clone, Event)]
struct CustomCreateDocumentEvent {
    document_id: String,
    collection_id: String,
    document_data: HashMap<String, Value>,
    id: usize,
}

impl CreateDocumentEventBuilder for CustomCreateDocumentEvent {
    fn collection_id(&self) -> String {
        self.collection_id.clone()
    }
    fn document_data(&self) -> HashMap<String, Value> {
        self.document_data.clone()
    }
    fn document_id(&self) -> String {
        self.document_id.clone()
    }
    fn new(options: Self) -> Self {
        options
    }
    fn id(&self) -> usize {
        self.id
    }
}

#[derive(Clone, Event)]
struct CustomCreateDocumentResponseEvent {
    result: DocumentResult,
    id: usize,
}

impl CreateDocumentResponseEventBuilder for CustomCreateDocumentResponseEvent {
    fn new(result: DocumentResult, id: usize) -> Self {
        CustomCreateDocumentResponseEvent { result, id }
    }
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
        // Add custom events
        .add_event::<CustomCreateDocumentEvent>()
        .add_event::<CustomCreateDocumentResponseEvent>()
        // Register handlers for custom events
        .add_systems(
            Update,
            create_document_event_handler::<
                CustomCreateDocumentEvent,
                CustomCreateDocumentResponseEvent,
            >
                .run_if(in_state(FirestoreState::Ready)),
        )
        .add_systems(Update, custom_create_document_response_event_handler)
        // Test fns
        .add_systems(Update, input)
        .add_systems(OnEnter(FirestoreState::Ready), create_test_document)
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

fn custom_create_document_response_event_handler(
    mut er: EventReader<CustomCreateDocumentResponseEvent>,
) {
    for e in er.iter() {
        match e.result.clone() {
            Ok(result) => {
                println!("Custom Event: {} Document created: {:?}", e.id, result);
            }
            Err(status) => {
                println!(
                    "Custom Event: {} ERROR: Document create failed: {}",
                    e.id, status
                );
            }
        }
    }
}

fn create_test_document(mut document_creator: EventWriter<CustomCreateDocumentEvent>) {
    let document_id = "test_document".to_owned();

    let mut document_data = HashMap::new();

    document_data.insert(
        "test_field".to_string(),
        Value {
            value_type: Some(ValueType::IntegerValue(69)),
        },
    );

    document_creator.send(CustomCreateDocumentEvent {
        document_id,
        collection_id: "test_collection".into(),
        document_data,
        id: 0b1000101,
    });
}
