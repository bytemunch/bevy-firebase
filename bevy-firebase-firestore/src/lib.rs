mod googleapis;

// re-exports
// TODO find out if this is the right way of doing things
pub mod deps {
    use crate::googleapis;

    pub use googleapis::google::firestore::v1::*;
    pub use tonic::Status;
}

use std::fs::read_to_string;
use std::{collections::HashMap, path::PathBuf};

use crate::googleapis::google::firestore::v1::{
    firestore_client::FirestoreClient,
    listen_request::TargetChange,
    target::{DocumentsTarget, TargetType},
    CreateDocumentRequest, DeleteDocumentRequest, Document, GetDocumentRequest, ListenRequest,
    ListenResponse, Target, UpdateDocumentRequest, Value,
};

use bevy::prelude::*;

use bevy_firebase_auth::{AuthState, ProjectId, TokenData};
use bevy_tokio_tasks::TokioTasksRuntime;

use futures_lite::{stream, StreamExt};

use tonic::{
    codegen::InterceptedService,
    metadata::{Ascii, MetadataValue},
    service::Interceptor,
    transport::{Certificate, Channel, ClientTlsConfig},
    Request, Response, Status,
};

// FIRESTORE
#[derive(Resource, Clone)]
pub struct BevyFirestoreClient(
    pub FirestoreClient<InterceptedService<Channel, FirebaseInterceptor>>,
);

#[derive(Resource, Clone)]
struct EmulatorUrl(String);

#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
pub enum FirestoreState {
    #[default]
    Start,
    Init,
    CreateClient,
    Ready,
}

#[derive(Clone)]
pub struct FirebaseInterceptor {
    bearer_token: MetadataValue<Ascii>,
    db: MetadataValue<Ascii>,
}

impl Interceptor for FirebaseInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> Result<tonic::Request<()>, tonic::Status> {
        request
            .metadata_mut()
            .insert("authorization", self.bearer_token.clone());

        request
            .metadata_mut()
            .insert("google-cloud-resource-prefix", self.db.clone());
        Ok(request)
    }
}

#[derive(Default)]
pub struct FirestorePlugin {
    pub emulator_url: Option<String>,
}

impl Plugin for FirestorePlugin {
    fn build(&self, app: &mut App) {
        // TODO refresh client token when app token is refreshed
        if self.emulator_url.is_some() {
            app.insert_resource(EmulatorUrl(self.emulator_url.clone().unwrap()));
        }

        app.add_state::<FirestoreState>()
            .add_system(logged_in.in_schedule(OnEnter(AuthState::LoggedIn)))
            .add_system(init.in_schedule(OnEnter(FirestoreState::Init)))
            .add_system(create_client.in_schedule(OnEnter(FirestoreState::CreateClient)))
            // Events
            .add_event::<CreateDocumentEvent>()
            .add_event::<CreateDocumentResponseEvent>()
            // Event Readers
            .add_system(
                create_document_response_event_handler.in_set(OnUpdate(FirestoreState::Ready)),
            )
            .add_system(
                create_document_event_handler::<CreateDocumentEvent, CreateDocumentResponseEvent>
                    .in_set(OnUpdate(FirestoreState::Ready)),
            );
    }
}

fn logged_in(mut next_state: ResMut<NextState<FirestoreState>>) {
    next_state.set(FirestoreState::Init);
}

fn init(mut next_state: ResMut<NextState<FirestoreState>>) {
    next_state.set(FirestoreState::CreateClient);
}

fn create_client(
    runtime: ResMut<TokioTasksRuntime>,
    user_info: Res<TokenData>,
    emulator: Option<Res<EmulatorUrl>>,
    project_id: Res<ProjectId>,
) {
    let id_token = user_info.id_token.clone();
    let project_id = project_id.0.clone();

    let emulator_url = match emulator {
        Some(e) => Some(e.0.clone()),
        None => None,
    };

    // CREATE BG TASK TO INSERT CLIENT AS RESOURCE
    runtime.spawn_background_task(|mut ctx| async move {
        let data_dir = PathBuf::from_iter([std::env!("CARGO_MANIFEST_DIR"), "data"]);
        let certs = read_to_string(data_dir.join("gcp/gtsr1.pem")).unwrap();

        let channel = if emulator_url.is_none() {
            let tls_config = ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(certs))
                .domain_name("firestore.googleapis.com");

            Channel::from_static("https://firestore.googleapis.com")
                .tls_config(tls_config)
                .unwrap()
                .connect()
                .await
                .unwrap()
        } else {
            Channel::from_shared(emulator_url.unwrap())
                .unwrap()
                .connect()
                .await
                .unwrap()
        };

        let service = FirestoreClient::with_interceptor(
            channel,
            FirebaseInterceptor {
                bearer_token: format!("Bearer {}", id_token).parse().unwrap(),
                db: format!("projects/{}/databases/(default)", project_id.clone())
                    .parse()
                    .unwrap(),
            },
        );

        ctx.run_on_main_thread(move |ctx| {
            ctx.world.insert_resource(BevyFirestoreClient(service));

            ctx.world
                .insert_resource(NextState(Some(FirestoreState::Ready)));
        })
        .await;
    });
}

pub trait ListenerEventBuilder {
    fn new(msg: ListenResponse) -> Self;
}

pub fn add_listener<T>(
    runtime: &ResMut<TokioTasksRuntime>,
    client: &mut BevyFirestoreClient,
    project_id: String,
    target: String,
) where
    T: ListenerEventBuilder + std::marker::Send + std::marker::Sync + 'static,
{
    let mut client = client.0.clone();
    runtime.spawn_background_task(|mut ctx| async move {
        let db = format!("projects/{project_id}/databases/(default)");
        let req = ListenRequest {
            database: db.clone(),
            labels: HashMap::new(),
            target_change: Some(TargetChange::AddTarget(Target {
                target_id: 0x52757374, // rust in hex, for... reasons?
                once: false,
                resume_type: None,
                target_type: Some(TargetType::Documents(DocumentsTarget {
                    documents: vec![db + "/documents/" + &*target],
                })),
                ..Default::default()
            })),
        };

        let req = Request::new(stream::iter(vec![req]).chain(stream::pending()));

        // TODO handle errors
        let res = client.listen(req).await.unwrap();

        let mut res = res.into_inner();

        while let Some(msg) = res.next().await {
            ctx.run_on_main_thread(move |ctx| {
                ctx.world.send_event(T::new(msg.unwrap()));
            })
            .await;
        }
    });
}

// TODO make all of these event driven
pub type DocumentResponse = Result<Document, Status>;

pub trait CreateDocumentEventBuilder {
    fn new(options: Self) -> Self;
    fn document_id(&self) -> String {
        "".into()
    }
    fn collection_id(&self) -> String {
        "".into()
    }
    fn document_data(&self) -> HashMap<String, Value> {
        let h: HashMap<String, Value> = HashMap::new();
        h
    }
}

#[derive(Clone)]
pub struct CreateDocumentEvent {
    pub document_id: String,
    pub collection_id: String,
    pub document_data: HashMap<String, Value>,
}

impl CreateDocumentEventBuilder for CreateDocumentEvent {
    fn new(options: CreateDocumentEvent) -> Self {
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

#[derive(Clone)]
pub struct CreateDocumentResponseEvent {
    pub result: Result<Document, Status>,
}

pub trait DocResEventBuilder {
    fn new(result: DocumentResponse) -> Self;
}

impl DocResEventBuilder for CreateDocumentResponseEvent {
    fn new(result: DocumentResponse) -> Self {
        CreateDocumentResponseEvent { result }
    }
}

pub fn create_document_event_handler<T, R>(
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    mut er: EventReader<T>,
    runtime: ResMut<TokioTasksRuntime>,
    // TODO custom response event (passed in create event?)
) where
    R: DocResEventBuilder + Send + Sync + 'static + Clone,
    T: CreateDocumentEventBuilder + Send + Sync + 'static + Clone,
{
    for e in er.iter() {
        let mut client = client.0.clone();
        let project_id = project_id.0.clone();

        let collection_id = e.clone().collection_id().clone();
        let document_id = e.clone().document_id().clone();
        let fields = e.clone().document_data().clone();

        runtime.spawn_background_task(|mut ctx| async move {
            let response: Result<Response<Document>, Status> = client
                .create_document(CreateDocumentRequest {
                    parent: format!("projects/{project_id}/databases/(default)/documents",),
                    collection_id,
                    document_id,
                    document: Some(Document {
                        fields,
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .await;

            let result = match response {
                Ok(result) => Ok(result.into_inner()),
                Err(status) => Err(status),
            };

            ctx.run_on_main_thread(move |ctx| {
                ctx.world.send_event(R::new(result));
            })
            .await;
        });
    }
}

fn create_document_response_event_handler(mut er: EventReader<CreateDocumentResponseEvent>) {
    for e in er.iter() {
        match e.result.clone() {
            Ok(result) => {
                println!("Document created: {:?}", result)
            }
            Err(status) => {
                println!("ERROR: Document create failed: {}", status)
            }
        }
    }
}

pub async fn old_create_document(
    client: &mut BevyFirestoreClient,
    project_id: &String,
    document_id: &String,
    collection_id: &String,
    document_data: HashMap<String, Value>,
) -> Result<Response<Document>, Status> {
    client
        .0
        .create_document(CreateDocumentRequest {
            parent: format!("projects/{project_id}/databases/(default)/documents"),
            collection_id: collection_id.into(),
            document_id: document_id.into(),
            document: Some(Document {
                fields: document_data,
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
}

pub async fn update_document(
    client: &mut BevyFirestoreClient,
    project_id: &String,
    document_path: &String,
    document_data: HashMap<String, Value>,
) -> Result<Response<Document>, Status> {
    client
        .0
        .update_document(UpdateDocumentRequest {
            document: Some(Document {
                name: format!(
                    "projects/{project_id}/databases/(default)/documents/{document_path}"
                ),
                fields: document_data,
                ..Default::default()
            }),
            // TODO update mask from document_data keys
            ..Default::default()
        })
        .await
}

pub async fn read_document(
    client: &mut BevyFirestoreClient,
    project_id: &String,
    document_path: &String,
) -> Result<Response<Document>, Status> {
    client
        .0
        .get_document(GetDocumentRequest {
            name: format!("projects/{project_id}/databases/(default)/documents/{document_path}"),
            ..Default::default()
        })
        .await
}

pub async fn delete_document(
    client: &mut BevyFirestoreClient,
    project_id: &String,
    document_path: &String,
) -> Result<Response<()>, Status> {
    client
        .0
        .delete_document(DeleteDocumentRequest {
            name: format!("projects/{project_id}/databases/(default)/documents/{document_path}"),
            ..Default::default()
        })
        .await
}
