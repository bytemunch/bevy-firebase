use std::collections::HashMap;

use bevy::prelude::*;
use bevy_firebase_auth::{log_in, log_out, GotAuthUrl, ProjectId, TokenData};
use bevy_firebase_firestore::{
    create_document_event_handler,
    deps::{listen_response::ResponseType, value::ValueType, ListenResponse, Status, Value},
    CreateDocumentEventBuilder, DocResEventBuilder, DocumentResponse, FirestoreState,
    ListenerEventBuilder,
    {add_listener, delete_document, read_document, update_document, BevyFirestoreClient},
};
use bevy_tokio_tasks::TokioTasksRuntime;

#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
enum AppAuthState {
    #[default]
    Setup,
    LogIn,
    LogOut,
}

struct MyListenerEvent {
    msg: ListenResponse,
}

impl ListenerEventBuilder for MyListenerEvent {
    fn new(msg: ListenResponse) -> Self {
        MyListenerEvent { msg }
    }
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
        // Custom Listener event
        .add_event::<MyListenerEvent>()
        .add_system(test_listener_system)
        // Test fns
        .add_system(input)
        // NEW EVENT BITS
        .add_event::<CustomCreateDocumentResponseEvent>()
        .add_event::<CustomCreateDocumentEvent>()
        .add_system(test_firestore_operations.in_schedule(OnEnter(FirestoreState::Ready)))
        .add_system(
            create_document_event_handler::<
                CustomCreateDocumentEvent,
                CustomCreateDocumentResponseEvent,
            >
                .in_set(OnUpdate(FirestoreState::Ready)),
        )
        .add_system(custom_create_document_response_event_handler)
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

fn test_listener_system(mut er: EventReader<MyListenerEvent>) {
    for ev in er.iter() {
        match ev.msg.response_type.as_ref().unwrap() {
            ResponseType::TargetChange(response) => {
                let change_type = response.target_change_type;

                // TODO match on googleapis::google::firestore::v1::target_change::TargetChangeType
                match change_type {
                    0 => {
                        // no change
                    }
                    1 => {
                        // target added
                    }
                    2 => {
                        // target removed
                    }
                    3 => {
                        // target current (research needed lol)
                    }
                    4 => {
                        // reset (also no idea)
                    }
                    _ => {
                        // unknown response
                    }
                }
            }
            ResponseType::DocumentChange(response) => {
                println!("Document Changed: {:?}", response.document.clone().unwrap());
            }
            ResponseType::DocumentDelete(response) => {
                println!("Document Deleted: {:?}", response.document.clone());
            }
            ResponseType::DocumentRemove(response) => {
                println!("Document Removed: {:?}", response.document.clone());
            }
            ResponseType::Filter(response) => {
                println!("Filter: {:?}", response);
            }
        }
    }
}

// TODO new example for custom events

#[derive(Clone)]
struct CustomCreateDocumentResponseEvent {
    result: DocumentResponse,
}

impl DocResEventBuilder for CustomCreateDocumentResponseEvent {
    fn new(result: DocumentResponse) -> Self {
        CustomCreateDocumentResponseEvent { result }
    }
}

#[derive(Clone)]
struct CustomCreateDocumentEvent {
    pub document_id: String,
    pub collection_id: String,
    pub document_data: HashMap<String, Value>,
}

impl CreateDocumentEventBuilder for CustomCreateDocumentEvent {
    fn new(options: CustomCreateDocumentEvent) -> Self {
        options
    }
    fn collection_id(&self) -> String {
        self.collection_id.clone()
    }
    fn document_data(&self) -> HashMap<String, Value> {
        self.document_data.clone()
    }
    fn document_id(&self) -> String {
        self.document_id.clone()
    }
}

fn custom_create_document_response_event_handler(
    mut er: EventReader<CustomCreateDocumentResponseEvent>,
) {
    for e in er.iter() {
        match e.result.clone() {
            Ok(result) => {
                println!("CUSTOM: Document created: {:?}", result)
            }
            Err(status) => {
                println!("CUSTOM: ERROR: Document create failed: {}", status)
            }
        }
    }
}

fn test_firestore_operations(
    client: ResMut<BevyFirestoreClient>,
    runtime: ResMut<TokioTasksRuntime>,
    project_id: Res<ProjectId>,
    user_info: Res<TokenData>,

    mut document_creator: EventWriter<CustomCreateDocumentEvent>,
) {
    let uid = user_info.local_id.clone();
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

    add_listener::<MyListenerEvent>(
        &runtime,
        &mut client,
        project_id.clone(),
        document_path.clone(),
    );

    document_creator.send(CustomCreateDocumentEvent {
        document_id: uid.clone(),
        collection_id: "lobbies".into(),
        document_data: data.clone(),
    });

    // TODO await document creation, then continue
    // TODO state machine for operations, modified in custom response handlers

    runtime.spawn_background_task(|mut ctx| async move {
        let document_path = &format!("lobbies/{}", uid);

        ctx.sleep_updates(90).await;

        let read = read_document(&mut client, &project_id, document_path).await;
        println!("READ 1: {:?}\n", read);

        data.insert(
            "test_field".into(),
            Value {
                value_type: Some(ValueType::IntegerValue(420)),
            },
        );

        data.insert(
            "another_field".into(),
            Value {
                value_type: Some(ValueType::StringValue("Ananas".into())),
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
