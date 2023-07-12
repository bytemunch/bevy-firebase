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
        // Custom create event listeners
        .add_event::<CustomCreateDocumentEvent>()
        .add_event::<CustomCreateDocumentResponseEvent>()
        .add_system(
            create_document_event_handler::<
                CustomCreateDocumentEvent,
                CustomCreateDocumentResponseEvent,
            >
                .in_set(OnUpdate(FirestoreState::Ready)),
        )
        .add_system(custom_create_document_response_event_handler)
        // Test fns
        .add_system(input)
        .add_system(create_test_document.in_schedule(OnEnter(FirestoreState::Ready)))
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

#[derive(Clone)]
struct CustomCreateDocumentEvent {
    document_id: String,
    collection_id: String,
    document_data: HashMap<String, Value>,
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
}

#[derive(Clone)]
struct CustomCreateDocumentResponseEvent {
    result: DocumentResult,
}

impl CreateDocumentResponseEventBuilder for CustomCreateDocumentResponseEvent {
    fn new(result: DocumentResult) -> Self {
        CustomCreateDocumentResponseEvent { result }
    }
}

fn custom_create_document_response_event_handler(
    mut er: EventReader<CustomCreateDocumentResponseEvent>,
) {
    for e in er.iter() {
        match e.result.clone() {
            Ok(result) => {
                println!("Custom Event: Document created: {:?}", result);
            }
            Err(status) => {
                println!("Custom Event: ERROR: Document create failed: {}", status);
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
    });
}
